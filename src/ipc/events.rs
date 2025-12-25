use serde::{Deserialize, Serialize};

use crate::ipc::snapshot::Snapshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Snapshot(Snapshot),
    PositionChanged(u64),
    PlaybackChanged,
    MetadataChanged,
    VolumeChanged,
}
