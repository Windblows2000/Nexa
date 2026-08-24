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
        Self { protocol_version: PROTOCOL_VERSION, client_name: client_name.into(), requested_features: Vec::new() }
    }
}
