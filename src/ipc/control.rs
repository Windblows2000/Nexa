use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Play,
    Pause,
    Next,
    Previous,
    Seek { position: f64 },
    SetVolume { value: f64 },
}
