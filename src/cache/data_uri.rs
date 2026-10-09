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

use std::path::PathBuf;

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose};
use tokio::fs;
use tracing::{info, trace};

use super::{
    hashing::stem_for_url,
    types::{ImageCache, MAX_OBJECT_BYTES},
};

impl ImageCache {
    pub async fn resolve_data_uri(&self, data_uri: &str) -> Result<PathBuf> {
        if let Some(path) = self.cached_path(data_uri).await {
            trace!(
                path = ?path,
                "Base64 art already exists in cache"
            );

            return Ok(path);
        }

        let (metadata, encoded) = data_uri.split_once(',').context("invalid data URI")?;

        if !metadata.contains(";base64") {
            anyhow::bail!("data URI is not base64 encoded");
        }

        let bytes = general_purpose::STANDARD.decode(encoded).context("failed to decode base64 image data")?;

        if bytes.is_empty() {
            anyhow::bail!("decoded image data was empty");
        }

        if bytes.len() as u64 > MAX_OBJECT_BYTES {
            anyhow::bail!(
                "decoded image exceeds maximum object size of \
{MAX_OBJECT_BYTES} bytes"
            );
        }

        let extension = infer::get(&bytes).map(|kind| kind.extension()).unwrap_or("bin");

        let stem = stem_for_url(data_uri);
        let final_path = self.root.join(format!("{stem}.{extension}"));

        fs::write(&final_path, &bytes).await?;

        info!(
            path = ?final_path,
            bytes = bytes.len(),
              "new Base64 art decoded and cached"
        );

        Ok(final_path)
    }
}
