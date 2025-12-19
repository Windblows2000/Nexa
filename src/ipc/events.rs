use serde::{Deserialize, Serialize};

use crate::ipc::snapshot::Snapshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Snapshot(Snapshot),
    PlaybackChanged,
    MetadataChanged,
    VolumeChanged,
}
