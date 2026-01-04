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

use crate::{
    daemon::state::{PlayerUpdate, SharedState},
    mpris::{PlayerStatus, SharedConnection, list_players, player_from_bus, snapshot_from_player},
    player::ActivityPriority,
};
use anyhow::Result;
use futures_util::StreamExt;
use tracing::{debug, trace, warn};
use zbus::fdo::DBusProxy;

pub async fn run(state: SharedState, conn: SharedConnection) -> Result<()> {
    debug!(target: "nexa::daemon::supervisor", "Initializing supervisor");

    if let Ok(buses) = list_players(&conn).await {
        for bus in buses {
            let state = state.clone();
            let conn = conn.clone();
            tokio::spawn(async move {
                if let Err(e) = monitor_player(state, conn, bus.clone()).await {
                    warn!(target: "nexa::daemon::supervisor", %bus, error = ?e, "Startup monitor exited");
                }
            });
        }
    }

    let raw = conn.clone();
    let dbus = DBusProxy::new(&raw).await?;
    let mut changes = dbus.receive_name_owner_changed().await?;

    while let Some(sig) = changes.next().await {
        let args = sig.args()?;
        let name = args.name();

        if name.starts_with("org.mpris.MediaPlayer2.") {
            if args.new_owner().is_some() {
                debug!(target: "nexa::daemon::supervisor", bus = %name, "New player detected");
                let state = state.clone();
                let conn = conn.clone();
                let bus_id = name.to_string();

                tokio::spawn(async move {
                    if let Err(e) = monitor_player(state, conn, bus_id.clone()).await {
                        warn!(target: "nexa::daemon::supervisor", bus = %bus_id, error = ?e, "Dynamic monitor exited");
                    }
                });
            } else {
                debug!(target: "nexa::daemon::supervisor", bus = %name, "Player left bus");
                state.remove_player(name).await;
            }
        }
    }
    Ok(())
}

async fn monitor_player(state: SharedState, conn: SharedConnection, bus: String) -> Result<()> {
    let proxy = player_from_bus(&conn, &bus).await?;

    if let Ok(snap) = snapshot_from_player(&proxy).await {
        state
            .upsert_snapshot_and_broadcast(snap, ActivityPriority::StatusUpdate, true)
            .await;
    }

    let mut status_changes = proxy.receive_playback_status_changed().await;
    let mut meta_changes = proxy.receive_metadata_changed().await;
    let mut seek_stream = proxy.receive_seeked().await?;

    loop {
        tokio::select! {
            res = status_changes.next() => {
                let Some(sig) = res else { break };
                if let Ok(new_status) = sig.get().await {
                    let update = PlayerUpdate {
                        status: Some(match new_status.as_str() {
                            "Playing" => PlayerStatus::Playing,
                            "Paused" => PlayerStatus::Paused,
                            _ => PlayerStatus::Stopped,
                        }),
                        ..Default::default()
                    };
                    state.apply_update_id_selective(&bus, update, ActivityPriority::StatusUpdate, true).await;
                }
            }
            res = meta_changes.next() => {
                if res.is_none() { break };
                if let Ok(snap) = snapshot_from_player(&proxy).await {
                    state.upsert_snapshot_and_broadcast(snap, ActivityPriority::StatusUpdate, true).await;
                }
            }
            res = seek_stream.next() => {
                let Some(sig) = res else {
                    debug!(target: "nexa::daemon::supervisor", %bus, "Seek stream closed");
                    break;
                };

                if let Ok(args) = sig.args() {
                    let new_pos = args.position();
                    trace!(target: "nexa::daemon::supervisor", %bus, pos = new_pos, "Seek detected");

                    let update = PlayerUpdate {
                        position_micros: Some(*new_pos),
                        ..Default::default()
                    };

                    state.apply_update_id_selective(
                        &bus,
                        update,
                        ActivityPriority::StatusUpdate,
                        true
                    ).await;
                }
            }
        }
    }

    debug!(target: "nexa::daemon::supervisor", %bus, "Removing player from state");
    state.remove_player(&bus).await;
    Ok(())
}
