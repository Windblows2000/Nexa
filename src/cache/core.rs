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

use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use tokio::{fs, sync::Mutex};

use super::types::ImageCache;

impl ImageCache {
    pub async fn new() -> Result<Self> {
        let proj = ProjectDirs::from("com", "windblows2000", "nexa").context("cannot determine cache dir")?;

        let root = proj.cache_dir().join("art");
        fs::create_dir_all(&root).await?;

        let client =
            reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent(concat!("nexa/", env!("CARGO_PKG_VERSION"))).build()?;

        Ok(Self { root, client, in_flight: Arc::new(Mutex::new(HashMap::new())) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
