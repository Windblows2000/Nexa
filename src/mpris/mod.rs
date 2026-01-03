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

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};
use zbus::names::BusName;
use zbus::{Connection, proxy, zvariant::Value};

pub type SharedConnection = std::sync::Arc<Mutex<Connection>>;

#[proxy(
    interface = "org.mpris.MediaPlayer2",
    default_path = "/org/mpris/MediaPlayer2"
)]
pub trait MprisRoot {
    fn raise(&self) -> zbus::Result<()>;
    fn quit(&self) -> zbus::Result<()>;
    fn can_quit(&self) -> zbus::Result<bool>;
    fn open_uri(&self, uri: &str) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
pub trait MprisPlayer {
    fn play(&self) -> zbus::Result<()>;
    fn pause(&self) -> zbus::Result<()>;
    fn play_pause(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    fn seek(&self, offset: i64) -> zbus::Result<()>;
    fn set_position(
        &self,
        track_id: zbus::zvariant::ObjectPath<'_>,
        position: i64,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, Value<'static>>>;

    #[zbus(property)]
    fn position(&self) -> zbus::Result<i64>;

    #[zbus(property)]
    fn volume(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn set_volume(&self, value: f64) -> zbus::Result<()>;

    #[zbus(property)]
    fn rate(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn set_rate(&self, value: f64) -> zbus::Result<()>;

    #[zbus(property)]
    fn shuffle(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_shuffle(&self, value: bool) -> zbus::Result<()>;

    #[zbus(property)]
    fn loop_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_loop_status(&self, value: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn seeked(&self, position: i64) -> zbus::Result<()>;
}

#[derive(Debug, Copy, Clone, Hash, Serialize, Eq, PartialEq)]
pub enum PlayerStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlayerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Default, Hash, PartialEq, Eq)]
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

pub async fn list_players(conn: &SharedConnection) -> Result<Vec<String>> {
    let conn = conn.lock().await.clone();
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let names = dbus.list_names().await?;

    let players = names
        .into_iter()
        .filter(|n| n.as_str().starts_with("org.mpris.MediaPlayer2."))
        .map(|n| n.to_string())
        .collect::<Vec<_>>();

    debug!(count = players.len(), "mpris players discovered");
    Ok(players)
}

fn parse_track_metadata(meta_map: &HashMap<String, Value<'static>>) -> TrackMetadata {
    let title = meta_map.get("xesam:title").and_then(|v| v.try_into().ok());
    let album = meta_map.get("xesam:album").and_then(|v| v.try_into().ok());
    let art_url = meta_map.get("mpris:artUrl").and_then(|v| v.try_into().ok());

    let track_id = meta_map.get("mpris:trackid").and_then(|v| {
        let p: zbus::zvariant::ObjectPath = v.try_into().ok()?;
        Some(p.to_string())
    });

    let length = meta_map.get("mpris:length").and_then(|v| match v {
        Value::I64(micros) if *micros >= 0 => Some(Duration::from_micros(*micros as u64)),
        Value::U64(micros) => Some(Duration::from_micros(*micros)),
        _ => None,
    });

    let artist = meta_map.get("xesam:artist").and_then(|v| {
        let arr: Vec<String> = match v {
            Value::Array(a) => a.iter().filter_map(|x| x.try_into().ok()).collect(),
            _ => return None,
        };
        Some(arr.join(", "))
    });

    TrackMetadata {
        title,
        artist,
        album,
        art_url,
        track_id,
        length,
    }
}

pub async fn snapshot_from_player(proxy: &MprisPlayerProxy<'_>) -> Result<PlayerStateSnapshot> {
    let player_id = proxy.inner().destination().to_string();
    trace!(player_id, "capturing snapshot");

    let status_raw = proxy.playback_status().await.unwrap_or_else(|e| {
        warn!(player_id, error = ?e, "playback_status fetch failed");
        "Stopped".to_string()
    });

    let status = match status_raw.as_str() {
        "Playing" => PlayerStatus::Playing,
        "Paused" => PlayerStatus::Paused,
        _ => PlayerStatus::Stopped,
    };

    let meta_map = proxy.metadata().await.unwrap_or_else(|e| {
        warn!(player_id, error = ?e, "metadata fetch failed");
        HashMap::new()
    });

    let metadata = parse_track_metadata(&meta_map);

    let position_micros = proxy.position().await.unwrap_or_else(|e| {
        trace!(player_id, error = ?e, "position unavailable");
        0
    });

    let position = Duration::from_micros(position_micros.max(0) as u64);

    Ok(PlayerStateSnapshot {
        player_id,
        status,
        metadata,
        position,
        rate: proxy.rate().await.ok(),
        volume: proxy.volume().await.ok(),
        shuffle: proxy.shuffle().await.ok(),
        loop_status: proxy.loop_status().await.ok(),
    })
}

pub async fn player_from_bus(
    conn: &SharedConnection,
    bus: &str,
) -> Result<MprisPlayerProxy<'static>> {
    let conn = conn.lock().await.clone();

    let bus_name =
        BusName::try_from(bus.to_owned()).with_context(|| format!("invalid bus name: {bus}"))?;

    MprisPlayerProxy::builder(&conn)
        .destination(bus_name)?
        .build()
        .await
        .context("failed to build player proxy")
}

pub async fn root_proxy(conn: &SharedConnection, bus: &str) -> Result<MprisRootProxy<'static>> {
    let conn = conn.lock().await.clone();

    let bus_name =
        BusName::try_from(bus.to_owned()).with_context(|| format!("invalid bus name: {bus}"))?;

    MprisRootProxy::builder(&conn)
        .destination(bus_name)?
        .build()
        .await
        .context("failed to build root proxy")
}
