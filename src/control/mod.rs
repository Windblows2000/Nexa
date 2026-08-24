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

pub async fn handle_request(req: Request, state: SharedState, conn: mpris::SharedConnection, cache: &ImageCache) -> Response {
    match handle_request_inner(req, state, conn, cache).await {
        Ok(resp) => resp,
        Err(e) => {
            warn!(error = %format!("{e:#}"), "Request failed");
            Response::Error(format!("{e:#}"))
        }
    }
}

pub(crate) async fn emit_snapshot(
    framed: &mut Framed<UnixStream, LengthDelimitedCodec>, snap: crate::mpris::PlayerStateSnapshot, cache: &ImageCache,
) -> Result<()> {
    let out = snapshot_out(snap, cache).await?;
    send(framed, Response::Metadata(Box::new(out))).await?;
    Ok(())
}

async fn handle_request_inner(req: Request, state: SharedState, conn: mpris::SharedConnection, cache: &ImageCache) -> Result<Response> {
    trace!(?req, "Handling request");

    match req {
        Request::Metadata { target } => {
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
            Ok(Response::List(apply_filter(ids, filter.as_deref())))
        }
        Request::Command { target, cmd } => {
            let msg = match target {
                Target::All { filter } => {
                    let ids = apply_filter(mpris::list_players(&conn).await?, filter.as_deref());
                    for id in ids {
                        execute_command(&state, conn.clone(), &id, &cmd).await.with_context(|| format!("command failed for {id}"))?;
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

fn apply_filter(ids: Vec<String>, filter: Option<&str>) -> Vec<String> {
    let Some(filter) = filter else {
        return ids;
    };

    let filter = filter.to_ascii_lowercase();
    ids.into_iter().filter(|id| id.to_ascii_lowercase().contains(&filter)).collect()
}

fn player_matches_filter(player_id: &str, filter: Option<&str>) -> bool {
    filter.map(|f| player_id.to_ascii_lowercase().contains(&f.to_ascii_lowercase())).unwrap_or(true)
}

async fn resolve_one_player(state: &SharedState, conn: &mpris::SharedConnection, target: Target) -> Result<String> {
    match target {
        Target::Player { id } => Ok(id),
        Target::Best { filter } => {
            if let Some(snap) = state.primary_snapshot().await
                && player_matches_filter(&snap.player_id, filter.as_deref())
            {
                return Ok(snap.player_id);
            }

            mpris::list_players(conn)
                .await?
                .into_iter()
                .find(|id| player_matches_filter(id, filter.as_deref()))
                .ok_or_else(|| anyhow!("No matching players found"))
        }
        Target::All { .. } => Err(anyhow!("Target::All invalid for single-player operations")),
    }
}

async fn get_snapshot_cached(state: &SharedState, conn: &mpris::SharedConnection, player_id: &str) -> Result<mpris::PlayerStateSnapshot> {
    if let Some(snapshot) = state.snapshot_for_player(player_id).await {
        return Ok(snapshot);
    }

    let proxy = mpris::player_from_bus(conn, player_id).await?;
    let snap = mpris::snapshot_from_player(&proxy).await?;
    state.upsert_snapshot_and_broadcast(snap.clone(), crate::player::ActivityPriority::MetadataUpdate, false).await;
    Ok(snap)
}

async fn execute_command(state: &SharedState, conn: mpris::SharedConnection, player_id: &str, cmd: &Command) -> Result<Option<String>> {
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
            let cur = player.volume().await.context("failed to query current volume")?;

            if level.is_none() && up.is_none() && down.is_none() {
                return Ok(Some(format!("{cur:.2}")));
            }

            let new = compute_volume(cur, *level, *up, *down).clamp(0.0, 1.0);
            player.set_volume(new).await?;
            Ok(None)
        }
        Command::Position { set_to, forward, backward } => {
            let current_pos = player.position().await.context("Failed to query player position for seeking")?;

            let track_id = if let Some(cached) = state.snapshot_for_player(player_id).await {
                cached.metadata.track_id
            } else {
                let meta = player.metadata().await?;
                meta.get("mpris:trackid").and_then(|v| v.try_into().ok())
            }
            .ok_or_else(|| anyhow!("Player has no TrackID"))?;

            let track_id_path = zbus::zvariant::ObjectPath::try_from(track_id)?;

            let new_pos = if let Some(sec) = set_to {
                (*sec as i64) * 1_000_000
            } else if let Some(sec) = forward {
                current_pos + (*sec as i64) * 1_000_000
            } else if let Some(sec) = backward {
                current_pos - (*sec as i64) * 1_000_000
            } else {
                let total_secs = (current_pos.max(0) as f64 / 1_000_000.0).round() as u64;
                return Ok(Some(format_duration(total_secs)));
            };

            player.set_position(track_id_path, new_pos.max(0)).await?;
            Ok(None)
        }
        Command::Shuffle { state } => {
            let cur = player.shuffle().await.unwrap_or(false);
            let next = compute_shuffle(cur, state.as_ref().copied().unwrap_or(ShuffleState::Toggle));
            player.set_shuffle(next).await?;
            Ok(Some(if next { "On" } else { "Off" }.to_string()))
        }
        Command::Loop { state } => {
            let cur = player.loop_status().await.unwrap_or_else(|_| "None".to_string());
            let next = compute_loop(&cur, state.as_ref().copied().unwrap_or(LoopState::Toggle));
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

pub async fn snapshot_out(s: mpris::PlayerStateSnapshot, cache: &ImageCache) -> Result<PlayerSnapshotOut> {
    let mut art_url = s.metadata.art_url.clone();

    let art_path = match art_url.as_deref() {
        Some(u) if u.starts_with("data:") => {
            let path = if let Some(path) = cache.cached_path(u).await { Some(path) } else { cache.resolve_data_uri(u).await.ok() };

            if let Some(path) = &path {
                art_url = Url::from_file_path(path).ok().map(|url| url.to_string()).or_else(|| Some(path.to_string_lossy().into_owned()));
            } else {
                art_url = None;
            }

            path
        }

        Some(u) => match Url::parse(u).ok() {
            Some(url) if url.scheme() == "file" => url.to_file_path().ok().or_else(|| Some(std::path::PathBuf::from(url.path()))),

            Some(url) if matches!(url.scheme(), "http" | "https") => {
                let url = url.to_string();
                if let Some(path) = cache.cached_path(&url).await { Some(path) } else { cache.ensure_cached(&url).await.ok() }
            }

            _ => None,
        },

        None => None,
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
