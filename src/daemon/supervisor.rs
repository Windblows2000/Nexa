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
    cache::ImageCache,
    daemon::state::{PlayerUpdate, SharedState},
    mpris::{PlayerStatus, SharedConnection, list_players, player_from_bus, snapshot_from_player},
    player::ActivityPriority,
};
use anyhow::Result;
use futures_util::StreamExt;
use std::collections::HashMap;
use tracing::{debug, warn};
use url::Url;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::zvariant::Value;

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

    if let Ok(mut snap) = snapshot_from_player(&proxy).await {
        snap.metadata.art_url = prepare_art_url_for_state(&state.cache, snap.metadata.art_url.take()).await;

        state.upsert_snapshot_and_broadcast(snap, ActivityPriority::StatusUpdate, true).await;
    }

    let props_proxy = PropertiesProxy::builder(&conn).destination(bus.clone())?.path("/org/mpris/MediaPlayer2")?.build().await?;

    let mut property_changes = props_proxy.receive_properties_changed().await?;
    let mut seek_stream = proxy.receive_seeked().await?;

    loop {
        tokio::select! {
            res = property_changes.next() => {
                let Some(sig) = res else { break };
                let args = sig.args()?;
                let changed = args.changed_properties();

                let mut update = PlayerUpdate::default();
                let mut has_changes = false;

                if let Some(val) = changed.get("Metadata")
                    && let Ok(dict) = <HashMap<String, Value>>::try_from(val.clone()) {
                        let mut meta = crate::mpris::parse_track_metadata(&dict);

                        meta.art_url =
                        prepare_art_url_for_state(&state.cache, meta.art_url.take()).await;

                        update.metadata = Some(meta);
                        has_changes = true;
                    }

                    if let Some(val) = changed.get("PlaybackStatus")
                        && let Ok(s) = <&str>::try_from(val) {
                            update.status = Some(match s {
                                "Playing" => PlayerStatus::Playing,
                                "Paused" => PlayerStatus::Paused,
                                _ => PlayerStatus::Stopped,
                            });
                            has_changes = true;
                        }

                        if let Some(val) = changed.get("Volume")
                            && let Ok(v) = f64::try_from(val) {
                                update.volume = Some(v);
                                has_changes = true;
                            }

                            if let Some(val) = changed.get("Shuffle")
                                && let Ok(b) = bool::try_from(val) {
                                    update.shuffle = Some(b);
                                    has_changes = true;
                                }

                                if let Some(val) = changed.get("LoopStatus")
                                    && let Ok(s) = <&str>::try_from(val) {
                                        update.loop_status = Some(s.to_string());
                                        has_changes = true;
                                    }

                                    if has_changes {
                                        state.apply_update_id_selective(&bus, update, ActivityPriority::StatusUpdate, true).await;
                                    }
            }
            res = seek_stream.next() => {
                let Some(sig) = res else { break };
                if let Ok(args) = sig.args() {
                    let update = PlayerUpdate {
                        position_micros: Some(*args.position()),
                        ..Default::default()
                    };
                    state.apply_update_id_selective(&bus, update, ActivityPriority::StatusUpdate, true).await;
                }
            }
        }
    }

    state.remove_player(&bus).await;
    Ok(())
}

async fn resolve_any_art(cache: &ImageCache, uri: &str) -> Result<std::path::PathBuf> {
    if uri.starts_with("data:") {
        cache.resolve_data_uri(uri).await
    } else if uri.starts_with("http") {
        cache.ensure_cached(uri).await
    } else {
        Ok(std::path::PathBuf::from(uri.trim_start_matches("file://")))
    }
}

async fn prewarm_art_cache(cache: &ImageCache, uri: Option<&str>) {
    let Some(uri) = uri else {
        return;
    };

    if let Err(err) = resolve_any_art(cache, uri).await {
        debug!(art_url = %uri, error = %err, "Failed to prewarm album art cache");
    }
}

async fn prepare_art_url_for_state(cache: &ImageCache, art_url: Option<String>) -> Option<String> {
    let uri = art_url?;

    if uri.starts_with("data:") {
        match cache.resolve_data_uri(&uri).await {
            Ok(path) => {
                if let Ok(file_url) = Url::from_file_path(&path) {
                    Some(file_url.to_string())
                } else {
                    warn!(
                        art_path = ?path,
                        "Decoded data URI album art but failed to convert path to file URL"
                    );
                    Some(path.to_string_lossy().into_owned())
                }
            }
            Err(err) => {
                warn!(error = %err, original_len = uri.len(), "Failed to decode data URI album art");
                None
            }
        }
    } else {
        prewarm_art_cache(cache, Some(&uri)).await;
        Some(uri)
    }
}
