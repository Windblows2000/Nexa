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

use crate::ipc::send;
use crate::output::format_duration;
use crate::{
    cache::ImageCache,
    daemon::state::SharedState,
    ipc::{Command, LoopState, PlayerSnapshotOut, Request, Response, ShuffleState, Target},
    mpris,
};
use anyhow::{Context, Result, anyhow};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{trace, warn};
use url::Url;

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
    send(framed, Response::Metadata(Box::new(out))).await?;
    Ok(())
}

async fn handle_request_inner(
    req: Request,
    state: SharedState,
    conn: mpris::SharedConnection,
    cache: &ImageCache,
) -> Result<Response> {
    trace!(?req, "Handling request");

    match req {
        Request::Metadata { target, .. } => {
            let id = resolve_one_player(&state, &conn, target).await?;
            let snap = get_snapshot_cached(&state, &conn, &id).await?;
            let out = snapshot_out(snap, cache).await?;
            Ok(Response::Metadata(Box::new(out)))
        }

        Request::Status { target } => {
            let id = resolve_one_player(&state, &conn, target).await?;
            let snap = get_snapshot_cached(&state, &conn, &id).await?;
            Ok(Response::Status(snap.status.as_str().to_string()))
        }

        Request::List { filter } => {
            let ids = mpris::list_players(&conn).await?;
            let out = apply_filter(ids, filter);
            Ok(Response::List(out))
        }

        Request::Command { target, cmd } => {
            let msg = match target {
                Target::All { filter } => {
                    let ids = apply_filter(mpris::list_players(&conn).await?, filter);
                    for id in ids {
                        execute_command(&state, conn.clone(), &id, &cmd)
                            .await
                            .with_context(|| format!("command failed for {id}"))?;
                    }
                    None
                }
                _ => {
                    let id = resolve_one_player(&state, &conn, target).await?;
                    execute_command(&state, conn.clone(), &id, &cmd).await?
                }
            };
            Ok(Response::Ok(msg))
        }

        Request::Ping => Ok(Response::Pong),
        Request::Follow { .. } => Ok(Response::Error("Follow is handled as a stream".into())),
    }
}

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
            if let Some(snap) = state.primary_snapshot().await {
                if let Some(f) = filter.as_deref() {
                    if snap.player_id.to_lowercase().contains(&f.to_lowercase()) {
                        return Ok(snap.player_id);
                    }
                } else {
                    return Ok(snap.player_id);
                }
            }

            let mut ids = mpris::list_players(conn).await?;
            if let Some(f) = filter {
                let f = f.to_lowercase();
                ids.retain(|id| id.to_lowercase().contains(&f));
            }

            ids.into_iter()
                .next()
                .ok_or_else(|| anyhow!("No matching players found"))
        }
        Target::All { .. } => Err(anyhow!("Target::All invalid for single-player operations")),
    }
}

async fn get_snapshot_cached(
    state: &SharedState,
    conn: &mpris::SharedConnection,
    player_id: &str,
) -> Result<mpris::PlayerStateSnapshot> {
    if let Some(s) = state.snapshot_for_player(player_id).await {
        return Ok(s);
    }

    let proxy = mpris::player_from_bus(conn, player_id).await?;
    let snap = mpris::snapshot_from_player(&proxy).await?;

    state
        .upsert_snapshot_and_broadcast(
            snap.clone(),
            crate::player::ActivityPriority::MetadataUpdate,
            false,
        )
        .await;
    Ok(snap)
}

async fn execute_command(
    state: &SharedState,
    conn: mpris::SharedConnection,
    player_id: &str,
    cmd: &Command,
) -> Result<Option<String>> {
    let player = mpris::player_from_bus(&conn, player_id).await?;

    match cmd {
        Command::Play => {
            player.play().await?;
            Ok(None)
        }
        Command::Pause => {
            player.pause().await?;
            Ok(None)
        }
        Command::PlayPause => {
            player.play_pause().await?;
            Ok(None)
        }
        Command::Next => {
            player.next().await?;
            Ok(None)
        }
        Command::Previous => {
            player.previous().await?;
            Ok(None)
        }
        Command::Stop => {
            player.stop().await?;
            Ok(None)
        }

        Command::Open { uri } => {
            let proxy = mpris::root_proxy(&conn, player_id).await?;
            proxy.open_uri(uri).await?;
            Ok(None)
        }

        Command::Volume { level, up, down } => {
            let cur = player.volume().await.unwrap_or(0.0);

            if level.is_none() && up.is_none() && down.is_none() {
                return Ok(Some(format!("{:.2}", cur)));
            }

            let new = compute_volume(cur, *level, *up, *down).clamp(0.0, 1.0);
            player.set_volume(new).await?;
            Ok(None)
        }

        Command::Position {
            set_to,
            forward,
            backward,
        } => {
            if set_to.is_none() && forward.is_none() && backward.is_none() {
                let pos_micros = player.position().await.unwrap_or(0);
                let total_secs = (pos_micros as f64 / 1_000_000.0).round() as u64;
                return Ok(Some(format_duration(total_secs)));
            }

            if let Some(f) = forward {
                player.seek((*f as f64 * 1_000_000.0) as i64).await?;
            } else if let Some(b) = backward {
                player.seek((*b as f64 * -1_000_000.0) as i64).await?;
            } else if let Some(s) = set_to {
                let tid_str = if let Some(cached) = state.snapshot_for_player(player_id).await {
                    cached.metadata.track_id
                } else {
                    let meta = player.metadata().await?;
                    meta.get("mpris:trackid").and_then(|v| {
                        let op: zbus::zvariant::ObjectPath = v.try_into().ok()?;
                        Some(op.to_string())
                    })
                }
                .ok_or_else(|| anyhow!("Player has no TrackID"))?;

                let tid = zbus::zvariant::ObjectPath::try_from(tid_str)?;
                player
                    .set_position(tid, (*s as f64 * 1_000_000.0) as i64)
                    .await?;
            }
            Ok(None)
        }

        Command::Shuffle { state } => {
            let cur = player.shuffle().await.unwrap_or(false);
            let next_bool = compute_shuffle(cur, state.unwrap_or(ShuffleState::Toggle));
            player.set_shuffle(next_bool).await?;
            let msg = if next_bool { "On" } else { "Off" };
            Ok(Some(msg.to_string()))
        }

        Command::Loop { state } => {
            let cur_str = player
                .loop_status()
                .await
                .unwrap_or_else(|_| "None".to_string());

            let next = compute_loop(&cur_str, state.unwrap_or(LoopState::Toggle));
            player.set_loop_status(next.to_string()).await?;
            Ok(Some(next.to_string()))
        }
    }
}

fn compute_volume(cur: f64, level: Option<f64>, up: Option<f64>, down: Option<f64>) -> f64 {
    match (level, up, down) {
        (Some(v), _, _) => v,
        (_, Some(v), _) => cur + v,
        (_, _, Some(v)) => cur - v,
        _ => cur,
    }
}

fn compute_shuffle(cur: bool, state: ShuffleState) -> bool {
    match state {
        ShuffleState::On => true,
        ShuffleState::Off => false,
        ShuffleState::Toggle => !cur,
    }
}

fn compute_loop(cur: &str, state: LoopState) -> &'static str {
    match state {
        LoopState::None => "None",
        LoopState::Track => "Track",
        LoopState::Playlist => "Playlist",
        LoopState::Toggle => match cur {
            "Track" => "Playlist",
            "Playlist" => "None",
            _ => "Track",
        },
    }
}

pub async fn snapshot_out(
    s: mpris::PlayerStateSnapshot,
    cache: &ImageCache,
) -> Result<PlayerSnapshotOut> {
    let art_url = s.metadata.art_url.clone();
    let art_path = match art_url.as_deref() {
        Some(u) if u.starts_with("file://") => Url::parse(u)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .or_else(|| Some(std::path::PathBuf::from(u.trim_start_matches("file://")))),
        Some(u) if u.starts_with("http") => {
            if let Some(p) = cache.cached_path(u).await {
                Some(p)
            } else {
                cache.ensure_cached(u).await.ok()
            }
        }
        _ => None,
    };

    Ok(PlayerSnapshotOut {
        player_id: s.player_id,
        identity: s.identity,
        status: s.status.as_str().to_string(),
        title: s.metadata.title,
        artist: s.metadata.artist,
        album: s.metadata.album,
        art_url,
        art_path,
        elapsed: s.position.as_secs(),
        length: s.metadata.length.map(|d| d.as_secs()),
        rate: s.rate,
        volume: s.volume,
        shuffle: s.shuffle,
        loop_status: s.loop_status,
    })
}
