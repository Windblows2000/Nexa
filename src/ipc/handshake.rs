use serde::{Deserialize, Serialize};

use crate::ipc::capabilities::Feature;
use crate::ipc::version::PROTOCOL_VERSION;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handshake {
    pub protocol_version: u16,
    pub client_name: String,
    pub requested_features: Vec<Feature>,
}

impl Handshake {
    pub fn new(client_name: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            client_name: client_name.into(),
            requested_features: Vec::new(),
        }
    }
}
