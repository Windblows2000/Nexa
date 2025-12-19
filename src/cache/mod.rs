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

use anyhow::{Context, Result};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::{fs, io::AsyncWriteExt};

const MAX_CACHE_BYTES: u64 = 1_000_000_000;

#[derive(Clone)]
pub struct ImageCache {
    root: PathBuf,
    client: reqwest::Client,
}

impl ImageCache {
    pub fn new() -> Result<Self> {
        let proj = ProjectDirs::from("com", "windblows2000", "nexa")
        .context("cannot determine cache dir")?;

        let root = proj.cache_dir().join("art");

        Ok(Self {
            root,
            client: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?,
        })
    }

    /// Return the cache root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return (file_count, total_size_bytes).
    pub async fn stats(&self) -> Result<(usize, u64)> {
        let mut count = 0;
        let mut size = 0;

        let mut rd = match fs::read_dir(&self.root).await {
            Ok(rd) => rd,
            Err(_) => return Ok((0, 0)),
        };

        while let Some(e) = rd.next_entry().await? {
            let meta = e.metadata().await?;
            if meta.is_file() {
                count += 1;
                size += meta.len();
            }
        }

        Ok((count, size))
    }

    /// Remove all cached album art (best-effort).
    pub async fn clear(&self) -> Result<()> {
        let _ = fs::remove_dir_all(&self.root).await;
        Ok(())
    }

    /// Deterministic cache path for a given URL.
    pub fn path_for_url(&self, url: &str) -> PathBuf {
        let mut h = Sha256::new();
        h.update(url.as_bytes());
        let name = hex::encode(h.finalize());
        self.root.join(name)
    }

    /// Ensure the image for `url` exists in the cache and return its path.
    ///
    /// Guarantees:
    /// - Deterministic paths
    /// - Atomic writes
    /// - Size-only eviction (best-effort)
    pub async fn ensure_cached(&self, url: &str) -> Result<PathBuf> {
        let path = self.path_for_url(url);

        // ---- cache hit ----
        if fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // ---- download ----
        let bytes = self
        .client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

        // ---- atomic write ----
        let tmp = path.with_extension("tmp");
        let mut f = fs::File::create(&tmp).await?;
        f.write_all(&bytes).await?;
        f.flush().await?;
        drop(f);

        fs::rename(&tmp, &path).await?;

        // ---- eviction (best-effort) ----
        let _ = enforce_size_limit(&self.root).await;

        Ok(path)
    }
}

/// Enforce cache size limit by deleting oldest files until under limit.
///
/// Best-effort: failures are ignored.
async fn enforce_size_limit(root: &Path) -> Result<()> {
    let mut entries = Vec::new();
    let mut total_size: u64 = 0;

    let mut rd = match fs::read_dir(root).await {
        Ok(rd) => rd,
        Err(_) => return Ok(()),
    };

    while let Some(e) = rd.next_entry().await? {
        let meta = match e.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };

        if !meta.is_file() {
            continue;
        }

        let size = meta.len();
        let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        total_size += size;
        entries.push((mtime, e.path(), size));
    }

    if total_size <= MAX_CACHE_BYTES {
        return Ok(());
    }

    // Oldest first
    entries.sort_by_key(|(mtime, _, _)| *mtime);

    for (_, path, size) in entries {
        let _ = fs::remove_file(&path).await;
        total_size = total_size.saturating_sub(size);

        if total_size <= MAX_CACHE_BYTES {
            break;
        }
    }

    Ok(())
}
