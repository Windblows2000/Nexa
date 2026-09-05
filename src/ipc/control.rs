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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
