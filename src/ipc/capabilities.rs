use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Feature {
    MetadataFollow,
    AlbumArt,
    VolumeControl,
}
