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

use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt};

pub fn init_logging(verbosity: u8) -> Result<()> {
    let default_level = match verbosity {
        0 => "info",
        1 => "debug",
        2 => "trace",
        _ => "trace",
    };

    // Respect RUST_LOG if present; otherwise fall back to verbosity setting.
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    // Use try_init to avoid panicking if initialized multiple times (like in tests)
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();

    Ok(())
}
