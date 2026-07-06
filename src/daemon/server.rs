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
use futures_util::StreamExt;
use tracing::{debug, error, trace, warn};

use std::os::unix::fs::PermissionsExt;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::{
    cache::ImageCache,
    control::emit_snapshot,
    daemon::state::SharedState,
    ipc::{Request, Target, decode_request, send, socket_path},
};

struct TickDemandGuard {
    tx: tokio::sync::watch::Sender<usize>,
}

impl TickDemandGuard {
    fn new(tx: tokio::sync::watch::Sender<usize>) -> Self {
        tx.send_modify(|v| *v += 1);
        Self { tx }
    }
}

impl Drop for TickDemandGuard {
    fn drop(&mut self) {
        self.tx.send_modify(|v| *v = v.saturating_sub(1));
    }
}

pub async fn run(
    state: SharedState,
    conn: crate::mpris::SharedConnection,
    ticker_tx: tokio::sync::watch::Sender<usize>,
) -> Result<()> {
    let path = socket_path();

    if let Some(parent) = path.parent() {
        debug!(parent = ?parent, "Ensuring socket directory exists");
        tokio::fs::create_dir_all(parent).await?;
    }

    if path.exists() {
        debug!(path = ?path, "Cleaning up existing socket file");
        let _ = tokio::fs::remove_file(&path).await;
    }

    let listener = UnixListener::bind(&path)?;

    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&path, perms)?;

    debug!(path = ?path, "IPC server listening on Unix socket");

    loop {
        let (stream, addr) = listener.accept().await?;
        trace!(?addr, "Accepted new IPC connection");

        let state = state.clone();
        let conn = conn.clone();
        let ticker_tx = ticker_tx.clone();

        tokio::spawn(async move {
            if let Err(e) =
                handle_conn(state.clone(), conn.clone(), ticker_tx.clone(), stream).await
            {
                warn!(error = ?e, "IPC client connection error");
            }
        });
    }
}

async fn handle_conn(
    state: SharedState,
    conn: crate::mpris::SharedConnection,

    ticker_tx: tokio::sync::watch::Sender<usize>,
    stream: UnixStream,
) -> Result<()> {
    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(1024 * 1024)
        .new_codec();

    let mut framed = Framed::new(stream, codec);

    while let Some(frame_result) = framed.next().await {
        let bytes = match frame_result {
            Ok(b) => b,
            Err(e) => {
                error!(error = ?e, "Framing error");
                return Err(e.into());
            }
        };

        let req: Request = decode_request(&bytes)?;
        trace!(request = ?req, "Received IPC request");

        match req {
            Request::Follow { target, with_time } => {
                let _tick_guard = if with_time {
                    Some(TickDemandGuard::new(ticker_tx.clone()))
                } else {
                    None
                };

                debug!(?target, with_time, "Client entering Follow mode");
                handle_follow(state.clone(), &state.cache, &mut framed, target).await?;
                debug!("Client exited Follow mode");
                break;
            }

            _ => {
                let resp =
                    crate::control::handle_request(req, state.clone(), conn.clone(), &state.cache)
                        .await;
                send(&mut framed, resp).await?;
            }
        }
    }

    Ok(())
}

async fn handle_follow(
    state: SharedState,
    cache: &ImageCache,
    framed: &mut Framed<UnixStream, LengthDelimitedCodec>,
    target: Target,
) -> Result<()> {
    let mut rx = state.subscribe();
    let mut follow_state = crate::daemon::state::FollowState::default();

    if let Some(snap) = resolve_snapshot(&state, &target).await
        && follow_state.should_emit(&snap)
    {
        emit_snapshot(framed, snap, cache).await?;
    }

    loop {
        match rx.recv().await {
            Ok(snap) => {
                if !target_matches_snapshot(&target, &snap) {
                    continue;
                }

                if matches!(target, Target::Best { .. })
                    && let Some(primary) = state.primary_snapshot().await
                    && primary.player_id != snap.player_id
                {
                    continue;
                }

                if follow_state.should_emit(&snap) {
                    trace!(player_id = %snap.player_id, "Emitting change-based snapshot");
                    emit_snapshot(framed, snap, cache).await?;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                warn!(missed = count, "Client lagged");
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
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
