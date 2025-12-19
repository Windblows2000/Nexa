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
use mprizzle::{Mpris, MprisEvent};
use std::time::Duration;

use crate::{
    daemon::state::{PlayerUpdate, SharedState},
    mpris,
    player::ActivityPriority,
};

pub async fn run(state: SharedState, mpris: &mut Mpris) -> Result<()> {
    let conn = mpris.connection();

    mpris.watch();

    if let Ok(existing_buses) = mpris::list_players(conn.clone()).await {
        for bus in existing_buses {
            if let Ok(p) = mpris::player_from_bus(conn.clone(), &bus).await
                && let Ok(snap) = mpris::snapshot_from_player(&p).await
            {
                state
                    .upsert_snapshot(snap, ActivityPriority::MetadataUpdate)
                    .await;
            }
        }
    }

    loop {
        let evt = match mpris.recv().await? {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(error = ?e, "skipping mpris event error");
                continue;
            }
        };

        match evt {
            MprisEvent::PlayerAttached(player) => {
                if let Ok(snap) = mpris::snapshot_from_player(&player).await {
                    state
                        .upsert_snapshot(snap, ActivityPriority::MetadataUpdate)
                        .await;
                }
            }

            MprisEvent::PlayerDetached(identity) => {
                state.remove_player_id(&identity).await;
            }

            MprisEvent::PlayerPropertiesChanged(identity) => {
                if let Ok(p) = mpris::player_from_bus(conn.clone(), identity.bus()).await
                    && let Ok(snap) = mpris::snapshot_from_player(&p).await
                {
                    let prio = match snap.status {
                        mpris::PlayerStatus::Playing | mpris::PlayerStatus::Paused => {
                            ActivityPriority::StatusUpdate
                        }
                        _ => ActivityPriority::MetadataUpdate,
                    };
                    state.upsert_snapshot(snap, prio).await;
                }
            }

            MprisEvent::PlayerSeeked(identity) => {
                if let Ok(p) = mpris::player_from_bus(conn.clone(), identity.bus()).await {
                    let upd = PlayerUpdate {
                        position_micros: p.position().await.ok().map(|d| d.as_micros() as i64),
                        ..Default::default()
                    };
                    state
                        .apply_update_id(&identity, upd, ActivityPriority::StatusUpdate)
                        .await;
                }
            }

            MprisEvent::PlayerPosition(identity, pos) => {
                let upd = PlayerUpdate {
                    position_micros: Some(pos.as_micros() as i64),
                    ..Default::default()
                };
                state
                    .apply_update_id(&identity, upd, ActivityPriority::StatusUpdate)
                    .await;
            }
        }

        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
