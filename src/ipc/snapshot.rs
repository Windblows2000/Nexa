use serde::{Deserialize, Serialize};

use crate::ipc::capabilities::Feature;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Volume {
    pub level: f32, // 0.0 – 1.0
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerCapabilities {
    pub features: Vec<Feature>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub playback: PlaybackState,
    pub metadata: Metadata,
    pub volume: Volume,
    pub capabilities: PlayerCapabilities,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            playback: PlaybackState::Stopped,
            metadata: Metadata {
                title: None,
                artist: None,
                album: None,
            },
            volume: Volume {
                level: 1.0,
                muted: false,
            },
            capabilities: PlayerCapabilities { features: vec![] },
        }
    }
}
