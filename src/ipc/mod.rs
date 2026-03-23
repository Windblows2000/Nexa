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

//! Public IPC API (v1).
//!
//! This module defines the stable, additive-only protocol
//! used by external clients (CLI, GUI, third-party tools).

use crate::ipc::version::PROTOCOL_VERSION;
use anyhow::Result;
use directories::ProjectDirs;
use postcard;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

pub mod capabilities;
pub mod control;
pub mod events;
pub mod handshake;
pub mod snapshot;
mod transport;
pub mod version;

#[derive(Serialize)]
struct EnvelopeRef<'a, T> {
    version: u16,
    payload: &'a T,
}

#[derive(Deserialize)]
struct Envelope<T> {
    version: u16,
    payload: T,
}

pub async fn send(
    framed: &mut Framed<UnixStream, LengthDelimitedCodec>,
    resp: Response,
) -> Result<()> {
    transport::send(framed, &resp).await
}

pub fn encode_request(req: &Request) -> anyhow::Result<Vec<u8>> {
    let env = EnvelopeRef {
        version: PROTOCOL_VERSION,
        payload: req,
    };
    Ok(postcard::to_stdvec(&env)?)
}

pub fn decode_request(bytes: &[u8]) -> anyhow::Result<Request> {
    let env: Envelope<Request> = postcard::from_bytes(bytes)?;
    if env.version != PROTOCOL_VERSION {
        anyhow::bail!(
            "protocol mismatch: got {}, expected {}",
            env.version,
            PROTOCOL_VERSION
        );
    }
    Ok(env.payload)
}

pub fn encode_response(resp: &Response) -> anyhow::Result<Vec<u8>> {
    let env = EnvelopeRef {
        version: PROTOCOL_VERSION,
        payload: resp,
    };
    Ok(postcard::to_stdvec(&env)?)
}

pub fn decode_response(bytes: &[u8]) -> anyhow::Result<Response> {
    let env: Envelope<Response> = postcard::from_bytes(bytes)?;
    if env.version != PROTOCOL_VERSION {
        anyhow::bail!(
            "protocol mismatch: got {}, expected {}",
            env.version,
            PROTOCOL_VERSION
        );
    }
    Ok(env.payload)
}

/// Location of the daemon's unix socket.
///
/// Uses $XDG_RUNTIME_DIR if available; otherwise falls back to a per-user cache dir.
pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("nexa").join("daemon.sock");
    }

    // Fall back to a stable per-user cache directory.
    if let Some(p) = ProjectDirs::from("", "", "nexa") {
        return p.cache_dir().join("daemon.sock");
    }

    PathBuf::from("/tmp/nexa-daemon.sock")
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum ShuffleState {
    On,
    Off,
    Toggle,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum LoopState {
    None,
    Track,
    Playlist,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,

    Open {
        uri: String,
    },

    /// Set absolute volume, or adjust relative.
    Volume {
        level: Option<f64>,
        up: Option<f64>,
        down: Option<f64>,
    },

    /// Seek / set position in microseconds (MPRIS units).
    Position {
        set_to: Option<u64>,
        forward: Option<u64>,
        backward: Option<u64>,
    },

    Shuffle {
        state: Option<ShuffleState>,
    },
    Loop {
        state: Option<LoopState>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Target {
    /// Explicit player bus name (e.g. org.mpris.MediaPlayer2.spotify).
    Player { id: String },
    /// Best available player (optionally filtered).
    Best { filter: Option<String> },
    /// Apply to all matching players.
    All { filter: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Ping,
    List {
        filter: Option<String>,
    },
    Status {
        target: Target,
    },
    Metadata {
        target: Target,
    },

    Command {
        target: Target,
        cmd: Command,
    },
    /// Stream updates; server will keep sending Metadata responses.
    Follow {
        target: Target,
        with_time: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Response {
    Pong,
    List(Vec<String>),
    Status(String),
    Metadata(Box<PlayerSnapshotOut>),
    Position(u64),
    Ok(Option<String>),
    Error(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerSnapshotOut {
    pub player_id: String,
    pub identity: String,
    pub status: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub art_url: Option<String>,
    pub art_path: Option<PathBuf>,
    pub elapsed: u64,
    pub length: Option<u64>,
    pub rate: Option<f64>,
    pub volume: Option<f64>,
    pub shuffle: Option<bool>,
    pub loop_status: Option<String>,
}
