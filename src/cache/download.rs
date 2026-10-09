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

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::{fs, io::AsyncWriteExt};
use tracing::{info, instrument, warn};
use url::Url;
use uuid::Uuid;

use super::{
    cleanup::enforce_size_limit,
    hashing::stem_for_url,
    types::{CACHE_EXTENSIONS, CLEANUP_TRIGGER_BYTES, ImageCache, MAX_OBJECT_BYTES, SNIFF_BYTES},
};

impl ImageCache {
    pub async fn cached_path(&self, url: &str) -> Option<PathBuf> {
        let stem = stem_for_url(url);

        for extension in CACHE_EXTENSIONS {
            let path = self.root.join(format!("{stem}.{extension}"));

            if self.is_valid_file(&path).await {
                return Some(path);
            }
        }

        None
    }

    #[instrument(skip(self), fields(url = %url))]
    pub async fn ensure_cached(&self, url: &str) -> Result<PathBuf> {
        if let Some(path) = self.cached_path(url).await {
            return Ok(path);
        }

        let lock = self.acquire_in_flight(url).await;

        let result = async {
            let _download_guard = lock.lock().await;

            // Another task may have populated the cache while this task
            // was waiting for the per-URL lock.
            if let Some(path) = self.cached_path(url).await {
                return Ok(path);
            }

            let stem = stem_for_url(url);
            self.download_to_cache(url, &stem).await
        }
        .await;

        self.release_in_flight(url, &lock).await;

        result
    }

    async fn is_valid_file(&self, path: &Path) -> bool {
        fs::metadata(path).await.map(|metadata| metadata.is_file() && metadata.len() > 0).unwrap_or(false)
    }

    async fn download_to_cache(&self, url: &str, stem: &str) -> Result<PathBuf> {
        let parsed = Url::parse(url).context("invalid art URL")?;

        match parsed.scheme() {
            "http" | "https" => {}
            other => anyhow::bail!("unsupported art URL scheme: {other}"),
        }

        let response = self.client.get(parsed).send().await?.error_for_status()?;

        if let Some(content_length) = response.content_length()
            && content_length > MAX_OBJECT_BYTES
        {
            anyhow::bail!("object too large: declared size is {content_length} bytes");
        }

        let temporary_path = self.root.join(format!("{stem}.tmp-{}", Uuid::new_v4()));

        let result = self.write_response_to_cache(response, stem, &temporary_path).await;

        if result.is_err() {
            let _ = fs::remove_file(&temporary_path).await;
        }

        result
    }

    async fn write_response_to_cache(&self, response: reqwest::Response, stem: &str, temporary_path: &Path) -> Result<PathBuf> {
        let mut file = fs::File::create(temporary_path).await?;
        let mut downloaded = 0u64;
        let mut sniff_buffer = Vec::with_capacity(SNIFF_BYTES);
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded += chunk.len() as u64;

            if downloaded > MAX_OBJECT_BYTES {
                anyhow::bail!("exceeded maximum object size of {MAX_OBJECT_BYTES} bytes");
            }

            if sniff_buffer.len() < SNIFF_BYTES {
                let remaining = SNIFF_BYTES - sniff_buffer.len();
                let amount = remaining.min(chunk.len());

                sniff_buffer.extend_from_slice(&chunk[..amount]);
            }

            file.write_all(&chunk).await?;
        }

        file.flush().await?;
        drop(file);

        if downloaded == 0 {
            anyhow::bail!("downloaded image was empty");
        }

        let extension = infer::get(&sniff_buffer).map(|kind| kind.extension()).unwrap_or("bin");

        let final_path = self.root.join(format!("{stem}.{extension}"));

        if let Err(error) = fs::rename(temporary_path, &final_path).await {
            let _ = fs::remove_file(temporary_path).await;
            return Err(error.into());
        }

        info!(path = ?final_path, bytes = downloaded, "cached image");

        if downloaded >= CLEANUP_TRIGGER_BYTES {
            let root = self.root.clone();

            tokio::spawn(async move {
                if let Err(error) = enforce_size_limit(&root).await {
                    warn!(error = %error, "cache cleanup failed");
                }
            });
        }

        Ok(final_path)
    }
}
