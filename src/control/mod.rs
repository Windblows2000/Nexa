// Copyright (C) 2025 Windblows2000
// This file is part of nexa.
//
// nexa is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::ipc::transport::send;
use crate::{
    cache::ImageCache,
    daemon::state::SharedState,
    ipc::{Command, LoopState, PlayerSnapshotOut, Request, Response, ShuffleState, Target},
    mpris,
};
use anyhow::{Context, Result, anyhow};
use mprizzle::LoopStatus;
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, trace, warn};
use url::Url;

//
// ===== Public entry point =====
//

pub async fn handle_request(
    req: Request,
    state: SharedState,
    conn: mpris::SharedConnection,
    cache: &ImageCache,
) -> Response {
    match handle_request_inner(req, state, conn, cache).await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(error = %format!("{e:#}"), "Request failed");
            Response::Error(format!("{e:#}"))
        }
    }
}

pub(crate) async fn emit_snapshot(
    framed: &mut Framed<UnixStream, LengthDelimitedCodec>,
    snap: crate::mpris::PlayerStateSnapshot,
    cache: &ImageCache,
) -> Result<()> {
    let out = snapshot_out(snap, cache).await?;
    trace!(
        player = %out.player_id,
        art_url = ?out.art_url,
        art_path = ?out.art_path,
        "Sending metadata over IPC"
    );
    send(framed, Response::Metadata(Box::new(out))).await?;
    Ok(())
}

//
// ===== Core dispatcher =====
//

async fn handle_request_inner(
    req: Request,
    state: SharedState,
    conn: mpris::SharedConnection,
    cache: &ImageCache,
) -> Result<Response> {
    trace!(?req, "Handling request");

    match req {
        Request::Metadata { target, compat, .. } => {
            if compat.is_some() {
                warn!("Ignoring removed compat mode: IPC still carries compat field");
            }

            let id = resolve_one_player(&state, &conn, target).await?;
            debug!(player = %id, "Resolved player for metadata");

            let snap = get_snapshot_cached(&state, &conn, &id).await?;
            let out = snapshot_out(snap, cache).await?;
            Ok(Response::Metadata(Box::new(out)))
        }

        Request::Status { target } => {
            let id = resolve_one_player(&state, &conn, target).await?;
            debug!(player = %id, "Resolved player for status");

            let snap = get_snapshot_cached(&state, &conn, &id).await?;
            Ok(Response::Status(snap.status.as_str().to_string()))
        }

        Request::List { filter } => {
            let ids = mpris::list_players(conn.clone()).await?;
            let out = apply_filter(ids, filter);
            debug!(count = out.len(), "Listed players");
            Ok(Response::List(out))
        }

        Request::Command { target, cmd } => {
            debug!(?target, ?cmd, "Executing command");

            match target {
                Target::All { filter } => {
                    let ids = apply_filter(mpris::list_players(conn.clone()).await?, filter);
                    debug!(count = ids.len(), "Resolved Target::All");
                    for id in ids {
                        execute_command(&state, conn.clone(), &id, &cmd)
                            .await
                            .with_context(|| format!("command failed for {id}"))?;
                    }
                }
                _ => {
                    let id = resolve_one_player(&state, &conn, target).await?;
                    execute_command(&state, conn.clone(), &id, &cmd).await?;
                }
            }

            Ok(Response::Ok)
        }

        Request::Ping => Ok(Response::Pong),

        Request::Follow { .. } => Ok(Response::Error("Follow is handled as a stream".into())),
    }
}

//
// ===== Player resolution =====
//

fn apply_filter(mut ids: Vec<String>, filter: Option<String>) -> Vec<String> {
    let Some(f) = filter else { return ids };
    let f = f.to_lowercase();
    ids.retain(|id| id.to_lowercase().contains(&f));
    ids
}

async fn resolve_one_player(
    state: &SharedState,
    conn: &mpris::SharedConnection,
    target: Target,
) -> Result<String> {
    match target {
        Target::Player { id } => Ok(id),

        Target::Best { filter } => {
            // Prefer daemon's primary snapshot if available.
            if let Some(snap) = state.primary_snapshot().await {
                if let Some(f) = filter.as_deref() {
                    if snap.player_id.to_lowercase().contains(&f.to_lowercase()) {
                        debug!(player = %snap.player_id, "Best player resolved from primary snapshot (filtered)");
                        return Ok(snap.player_id);
                    }
                } else {
                    debug!(player = %snap.player_id, "Best player resolved from primary snapshot");
                    return Ok(snap.player_id);
                }
            }

            // Fallback: first matching player on the bus.
            let mut ids = mpris::list_players(conn.clone()).await?;
            if let Some(f) = filter {
                let f = f.to_lowercase();
                ids.retain(|id| id.to_lowercase().contains(&f));
            }

            let chosen = ids
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("No matching players found"))?;

            debug!(player = %chosen, "Best player resolved from bus listing");
            Ok(chosen)
        }

        Target::All { .. } => Err(anyhow!(
            "Target::All is not valid for single-player operations"
        )),
    }
}

async fn get_snapshot_cached(
    state: &SharedState,
    conn: &mpris::SharedConnection,
    player_id: &str,
) -> Result<mpris::PlayerStateSnapshot> {
    if let Some(s) = state.snapshot_for_player(player_id).await {
        trace!(player = %player_id, "Using cached snapshot");
        return Ok(s);
    }

    trace!(player = %player_id, "Snapshot not in cache; querying player");
    let player = mpris::player_from_bus(conn.clone(), player_id).await?;
    let snap = mpris::snapshot_from_player(&player).await?;

    state
        .upsert_snapshot(
            snap.clone(),
            crate::player::ActivityPriority::MetadataUpdate,
        )
        .await;

    Ok(snap)
}

//
// ===== Command execution =====
//

async fn execute_command(
    state: &SharedState,
    conn: mpris::SharedConnection,
    player_id: &str,
    cmd: &Command,
) -> Result<()> {
    let mut player = mpris::player_from_bus(conn.clone(), player_id).await?;
    trace!(player = %player_id, ?cmd, "Dispatching MPRIS command");

    match cmd {
        Command::Play => player.play().await?,
        Command::Pause => player.pause().await?,
        Command::PlayPause => player.play_pause().await?,
        Command::Next => player.next().await?,
        Command::Previous => player.previous().await?,
        Command::Stop => player.stop().await?,

        Command::Open { uri } => {
            debug!(player = %player_id, uri = %uri, "Calling OpenUri");
            let conn_ref = conn.lock().await.clone();
            let proxy = mpris::root_proxy(&conn_ref, player_id).await?;
            proxy.call_method("OpenUri", &(uri.clone(),)).await?;
        }

        Command::Volume { level, up, down } => {
            let cur = player.volume().await.unwrap_or(1.0);
            let new = compute_volume(cur, *level, *up, *down);

            trace!(
                player = %player_id,
                cur_volume = cur,
                new_volume = new,
                level = ?level,
                up = ?up,
                down = ?down,
                "Computed volume"
            );

            player.set_volume(new).await?;
        }

        Command::Position {
            set_to,
            forward,
            backward,
        } => {
            // CLI semantics: these are seconds. MPRIS: microseconds.
            // Convert once at the boundary; keep internal math in micros.
            let set_us = set_to.map(|s| s as i64 * 1_000_000);
            let forward_us = forward.map(|s| s as i64 * 1_000_000);
            let backward_us = backward.map(|s| s as i64 * 1_000_000);

            if set_us.is_none() {
                // Relative seek path (mprizzle helpers expect Duration)
                if let Some(f) = forward_us {
                    trace!(player = %player_id, forward_us = f, "Seeking forward");
                    player
                        .seek_forward(std::time::Duration::from_micros(f.max(0) as u64))
                        .await?;
                }
                if let Some(b) = backward_us {
                    trace!(player = %player_id, backward_us = b, "Seeking backward");
                    player
                        .seek_backward(std::time::Duration::from_micros(b.max(0) as u64))
                        .await?;
                }

                // If nothing was provided, that's user error, but historically this returns Ok.
                if forward_us.is_none() && backward_us.is_none() {
                    warn!(player = %player_id, "Position command called without --set/--forward/--backward");
                }

                return Ok(());
            }

            // Absolute set position path (requires track id)
            let cur = player.position().await.unwrap_or_default();
            let cur_micros = cur.as_micros() as i64;

            let new_micros = compute_position(cur_micros, set_us, forward_us, backward_us);
            let new_micros = new_micros.max(0);

            trace!(
                player = %player_id,
                cur_micros,
                set_us = ?set_us,
                forward_us = ?forward_us,
                    backward_us = ?backward_us,
                   new_micros,
                   "Computed position (microseconds)"
            );

            let new_pos = std::time::Duration::from_micros(new_micros as u64);

            let tid = if let Some(tid) = state
                .snapshot_for_player(player_id)
                .await
                .and_then(|s| s.metadata.track_id)
            {
                trace!(player = %player_id, track_id = %tid, "Using cached track id");
                tid
            } else {
                trace!(player = %player_id, "Track id not cached; querying metadata");
                let meta = player.metadata().await?;
                meta.track_id()?
                    .ok_or_else(|| anyhow!("player did not provide mpris:trackid"))?
                    .as_ref()
                    .to_string()
            };

            trace!(
                player = %player_id,
                track_id = %tid,
                new_pos_micros = new_micros,
                "Calling set_position"
            );

            player.set_position(&tid, new_pos).await?;
        }

        Command::Shuffle { state } => {
            let cur = player.shuffle().await.unwrap_or(false);
            let new = compute_shuffle(cur, *state);
            trace!(player = %player_id, cur, new, requested = ?state, "Computed shuffle");
            player.set_shuffle(new).await?;
        }

        Command::Loop { state } => {
            let cur = player.loop_status().await.unwrap_or(LoopStatus::None);
            let next = compute_loop(&cur, *state);

            trace!(
                player = %player_id,
                ?cur,
                ?next,
                requested = ?state,
                "Computed loop status"
            );

            player.set_loop_status(next).await?;
        }
    }

    Ok(())
}

//
// ===== Helpers =====
//

fn compute_volume(cur: f64, level: Option<f64>, up: Option<f64>, down: Option<f64>) -> f64 {
    match (level, up, down) {
        (Some(v), _, _) => v,
        (_, Some(v), _) => cur + v,
        (_, _, Some(v)) => cur - v,
        _ => cur,
    }
}

// All values are microseconds.
fn compute_position(
    cur: i64,
    set_to: Option<i64>,
    forward: Option<i64>,
    backward: Option<i64>,
) -> i64 {
    match (set_to, forward, backward) {
        (Some(v), _, _) => v,
        (_, Some(v), _) => cur + v,
        (_, _, Some(v)) => cur - v,
        _ => cur,
    }
}

fn compute_shuffle(cur: bool, state: Option<ShuffleState>) -> bool {
    match state {
        Some(ShuffleState::On) => true,
        Some(ShuffleState::Off) => false,
        Some(ShuffleState::Toggle) | None => !cur,
    }
}

fn compute_loop(cur: &LoopStatus, state: Option<LoopState>) -> LoopStatus {
    match state {
        Some(LoopState::None) => LoopStatus::None,
        Some(LoopState::Track) => LoopStatus::Track,
        Some(LoopState::Playlist) => LoopStatus::Playlist,
        None => match cur {
            LoopStatus::None => LoopStatus::Track,
            LoopStatus::Track => LoopStatus::Playlist,
            LoopStatus::Playlist => LoopStatus::None,
        },
    }
}

//
// ===== Snapshot conversion =====
//

pub async fn snapshot_out(
    s: mpris::PlayerStateSnapshot,
    cache: &ImageCache,
) -> Result<PlayerSnapshotOut> {
    trace!(
        player = %s.player_id,
        art_url = ?s.metadata.art_url,
        "Building PlayerSnapshotOut"
    );

    let art_url = s.metadata.art_url.clone();

    let art_path = match art_url.as_deref() {
        Some(u) => {
            trace!(art_url = u, "Attempting to resolve album art");

            if u.starts_with("file://") {
                trace!("Detected file:// album art");

                // strict parse
                match Url::parse(u) {
                    Ok(url) => match url.to_file_path() {
                        Ok(path) => {
                            trace!(path = %path.display(), "Resolved file:// album art via url::Url");
                            Some(path)
                        }
                        Err(_) => {
                            debug!("Url::to_file_path failed; falling back to prefix-strip");
                            fallback_file_path(u)
                        }
                    },
                    Err(e) => {
                        debug!(error = %e, "Url::parse failed; falling back to prefix-strip");
                        fallback_file_path(u)
                    }
                }
            } else if u.starts_with("http://") || u.starts_with("https://") {
                trace!("Detected remote album art; attempting cache");

                match cache.ensure_cached(u).await {
                    Ok(path) => {
                        trace!(path = %path.display(), "Cached remote album art successfully");
                        Some(path)
                    }
                    Err(e) => {
                        warn!(art_url = u, error = %e, "Failed to cache remote album art");
                        None
                    }
                }
            } else {
                debug!(art_url = u, "Unsupported album art scheme");
                None
            }
        }
        None => {
            trace!("No art_url present");
            None
        }
    };

    trace!(player = %s.player_id, art_path = ?art_path, "Resolved art_path");

    Ok(PlayerSnapshotOut {
        player_id: s.player_id,
        status: s.status.as_str().to_string(),
        title: s.metadata.title,
        artist: s.metadata.artist,
        album: s.metadata.album,
        art_url,
        art_path,
        elapsed_seconds: s.position.as_secs(),
        length_seconds: s.metadata.length.map(|d| d.as_secs()),
        rate: s.rate,
        volume: s.volume,
        shuffle: s.shuffle,
        loop_status: s.loop_status,
    })
}

fn fallback_file_path(u: &str) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(u.trim_start_matches("file://"));
    if path.is_absolute() {
        trace!(path = %path.display(), "Resolved file:// album art path (fallback)");
        Some(path)
    } else {
        debug!(art_url = u, "Fallback file:// path was not absolute");
        None
    }
}
