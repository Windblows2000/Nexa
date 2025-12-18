// Copyright (C) 2025 Windblows2000
// This file is part of rusty-player.
//
// rusty-player is free software: you can redistribute it and/or modify
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

use anyhow::{Context, Result};
use mprizzle::{LoopStatus, MprisPlayer, PlaybackStatus, PlayerIdentity, TrackId};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::Mutex;
use zbus::{Connection, Proxy, proxy::Builder as ProxyBuilder};

/// Shared DBus connection handle.
///
/// `mprizzle` stores the underlying `zbus::Connection` in this shape.
pub type SharedConnection = std::sync::Arc<Mutex<Connection>>;

const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";

#[derive(Debug, Copy, Clone, Serialize, Eq, PartialEq)]
pub enum PlayerStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlayerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PlayerStatus::Playing => "Playing",
            PlayerStatus::Paused => "Paused",
            PlayerStatus::Stopped => "Stopped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub art_url: Option<String>,
    pub track_id: Option<String>,
    pub length: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct PlayerStateSnapshot {
    pub player_id: String,
    pub status: PlayerStatus,
    pub metadata: TrackMetadata,
    pub position: Duration,
    pub rate: Option<f64>,
    pub volume: Option<f64>,
    pub shuffle: Option<bool>,
    pub loop_status: Option<String>,
}

/// Create an `mprizzle::MprisPlayer` for a specific bus name.
pub async fn player_from_bus(conn: SharedConnection, bus: &str) -> Result<MprisPlayer> {
    let identity = PlayerIdentity::new(bus.to_string())
        .with_context(|| format!("invalid MPRIS bus name: {bus}"))?;
    Ok(MprisPlayer::new(conn, identity).await?)
}

/// List MPRIS players by querying the bus.
///
/// This is *not* a fragile metadata parse: it's just a names listing.
pub async fn list_players(conn: SharedConnection) -> Result<Vec<String>> {
    let conn = conn.lock().await.clone();
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let names = dbus.list_names().await?;
    Ok(names
        .into_iter()
        .filter(|n| n.as_str().starts_with("org.mpris.MediaPlayer2."))
        .map(|n| n.to_string())
        .collect())
}

/// Build a daemon snapshot from an mprizzle player.
///
/// This uses `mprizzle`'s typed helpers for metadata/properties instead of
/// hand-parsing `zvariant` values.
pub async fn snapshot_from_player(p: &MprisPlayer) -> Result<PlayerStateSnapshot> {
    let player_id = p.identity().bus().to_string();

    let status = match p.playback_status().await? {
        PlaybackStatus::Playing => PlayerStatus::Playing,
        PlaybackStatus::Paused => PlayerStatus::Paused,
        _ => PlayerStatus::Stopped,
    };

    let meta = p.metadata().await?;
    let title = meta.title()?.map(|s| s.to_string());
    let album = meta.album()?.map(|s| s.to_string());
    let art_url = meta.art_url()?.map(|s| s.to_string());

    let artist = meta
        .artists()?
        .map(|artists| artists.into_iter().collect::<Vec<_>>().join(", "));

    let track_id = meta
        .track_id()?
        .as_ref()
        .map(|tid: &TrackId| tid.as_ref().to_string());

    let length = meta.length()?;

    let metadata = TrackMetadata {
        title,
        artist,
        album,
        art_url,
        track_id,
        length,
    };

    let position = p.position().await.unwrap_or(Duration::from_secs(0));
    let rate = p.playback_rate().await.ok();
    let volume = p.volume().await.ok();
    let shuffle = p.shuffle().await.ok();
    let loop_status = p.loop_status().await.ok().map(|ls| match ls {
        LoopStatus::None => "None".to_string(),
        LoopStatus::Track => "Track".to_string(),
        LoopStatus::Playlist => "Playlist".to_string(),
    });

    Ok(PlayerStateSnapshot {
        player_id,
        status,
        metadata,
        position,
        rate,
        volume,
        shuffle,
        loop_status,
    })
}

/// Create a proxy for the root `org.mpris.MediaPlayer2` interface.
///
/// This is used only for operations `mprizzle` intentionally does not model
/// (e.g. `OpenUri`).
pub async fn root_proxy<'a>(conn: &'a Connection, player_bus: &'a str) -> Result<Proxy<'a>> {
    Ok(ProxyBuilder::new(conn)
        .destination(player_bus)?
        .path(MPRIS_PATH)?
        .interface(ROOT_IFACE)?
        .build()
        .await?)
}
