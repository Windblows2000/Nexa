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

use anyhow::Result;
use futures::StreamExt;
use std::time::Duration;

use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::{
    cache::ImageCache,
    control::emit_snapshot,
    daemon::state::SharedState,
    ipc::{CompatMode, Request, Target, decode_request, send, socket_path},
};

pub async fn run(
    state: SharedState,
    conn: crate::mpris::SharedConnection,
    cache: ImageCache,
) -> Result<()> {
    let path = socket_path();

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(&path).await;

    let listener = UnixListener::bind(&path)?;

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        let conn = conn.clone();
        let cache = cache.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_conn(state, conn, cache, stream).await {
                tracing::warn!(error = ?e, "client error");
            }
        });
    }
}

async fn handle_conn(
    state: SharedState,
    conn: crate::mpris::SharedConnection,
    cache: ImageCache,
    stream: UnixStream,
) -> Result<()> {
    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

    while let Some(frame) = framed.next().await {
        let bytes = frame?;
        let req: Request = decode_request(&bytes)?;

        match req {
            Request::Follow {
                target,
                format,
                compat,
            } => {
                handle_follow(
                    state.clone(),
                    &cache,
                    &mut framed,
                    target,
                    format.as_deref(),
                    compat,
                )
                .await?;
                break;
            }
            _ => {
                let resp =
                    crate::control::handle_request(req, state.clone(), conn.clone(), &cache).await;
                send(&mut framed, resp).await?;
            }
        }
    }

    Ok(())
}

fn follow_tick(format: Option<&str>) -> Duration {
    match format {
        Some(f) if f.contains("{elapsed}") => Duration::from_millis(250),
        _ => Duration::from_millis(1000),
    }
}

async fn handle_follow(
    state: SharedState,
    cache: &ImageCache,
    framed: &mut Framed<UnixStream, LengthDelimitedCodec>,
    target: Target,
    format: Option<&str>,
    _compat: Option<CompatMode>,
) -> Result<()> {
    let mut rx = state.subscribe();
    let mut ticker = tokio::time::interval(follow_tick(format));

    if let Some(snap) = resolve_snapshot(&state, &target).await {
        emit_snapshot(framed, snap, cache).await?;
    }

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Some(snap) = resolve_snapshot(&state, &target).await {
                    emit_snapshot(framed, snap, cache).await?;
                }
            }
            msg = rx.recv() => {
                match msg {
                    Ok(snap) => {
                        if !target_matches_snapshot(&target, &snap) {
                            continue;
                        }
                        emit_snapshot(framed, snap, cache).await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}

async fn resolve_snapshot(
    state: &SharedState,
    target: &Target,
) -> Option<crate::mpris::PlayerStateSnapshot> {
    match target {
        Target::Player { id } => state.snapshot_for_player(id).await,
        Target::Best { filter } => {
            let snap = state.primary_snapshot().await?;
            if let Some(f) = filter {
                snap.player_id
                    .to_lowercase()
                    .contains(&f.to_lowercase())
                    .then_some(snap)
            } else {
                Some(snap)
            }
        }
        Target::All { .. } => state.primary_snapshot().await,
    }
}

fn target_matches_snapshot(target: &Target, snap: &crate::mpris::PlayerStateSnapshot) -> bool {
    match target {
        Target::Player { id } => id == &snap.player_id,
        Target::Best { filter } | Target::All { filter } => filter
            .as_ref()
            .map(|f| snap.player_id.to_lowercase().contains(&f.to_lowercase()))
            .unwrap_or(true),
    }
}
