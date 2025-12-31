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
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{fs, io::AsyncWriteExt, sync::Mutex};
use tracing::{info, instrument, warn};
use uuid::Uuid;

const MAX_CACHE_BYTES: u64 = 1_000_000_000;
const MAX_OBJECT_BYTES: u64 = 10 * 1024 * 1024;
const CLEANUP_TRIGGER_BYTES: u64 = MAX_CACHE_BYTES / 100;

#[derive(Clone)]
pub struct ImageCache {
    root: PathBuf,
    client: reqwest::Client,
    in_flight: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

struct InFlightGuard {
    url: String,
    map: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let map = self.map.clone();
        let url = self.url.clone();
        tokio::spawn(async move {
            map.lock().await.remove(&url);
        });
    }
}

impl ImageCache {
    pub async fn new() -> Result<Self> {
        let proj = ProjectDirs::from("com", "windblows2000", "nexa")
        .context("cannot determine cache dir")?;
        let root = proj.cache_dir().join("art");
        fs::create_dir_all(&root).await?;

        Ok(Self {
            root,
            client: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("nexa/1.0")
            .build()?,
           in_flight: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn stats(&self) -> Result<(usize, u64)> {
        let mut count = 0;
        let mut size = 0;

        let mut rd = match fs::read_dir(&self.root).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
            Err(e) => return Err(e.into()),
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

    pub async fn clear(&self) -> Result<()> {
        let _ = fs::remove_dir_all(&self.root).await;
        fs::create_dir_all(&self.root).await?;
        Ok(())
    }

    pub async fn cached_path(&self, url: &str) -> Option<PathBuf> {
        let path = self.path_for_url(url);
        if self.is_valid_file(&path).await {
            Some(path)
        } else {
            None
        }
    }

    #[instrument(skip(self), fields(url = %url))]
    pub async fn ensure_cached(&self, url: &str) -> Result<PathBuf> {
        let path = self.path_for_url(url);

        if self.is_valid_file(&path).await {
            return Ok(path);
        }

        let (lock, guard) = {
            let mut map = self.in_flight.lock().await;
            let lock = map
            .entry(url.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
            let guard = InFlightGuard {
                url: url.to_string(),
                map: self.in_flight.clone(),
            };
            (lock, guard)
        };

        let _lock_guard = lock.lock().await;
        let _cleanup = guard;

        if self.is_valid_file(&path).await {
            return Ok(path);
        }

        self.download_to_path(url, &path).await?;
        Ok(path)
    }

    async fn is_valid_file(&self, path: &Path) -> bool {
        fs::metadata(path)
        .await
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
    }

    async fn download_to_path(&self, url: &str, final_path: &Path) -> Result<()> {
        let resp = self.client.get(url).send().await?.error_for_status()?;

        if let Some(len) = resp.content_length() {
            if len > MAX_OBJECT_BYTES {
                anyhow::bail!("object too large: {} bytes", len);
            }
        }

        let ct_ok = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("image/"))
        .unwrap_or(false);

        if !ct_ok {
            anyhow::bail!("unexpected content type");
        }

        let tmp_path = final_path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        let mut file = fs::File::create(&tmp_path).await?;
        let mut downloaded: u64 = 0;
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded += chunk.len() as u64;

            if downloaded > MAX_OBJECT_BYTES {
                let _ = fs::remove_file(&tmp_path).await;
                anyhow::bail!("object exceeded max size during download");
            }

            file.write_all(&chunk).await?;
        }

        file.flush().await?;
        drop(file);

        fs::rename(&tmp_path, final_path).await?;
        info!(url = %url, "cached image");

        if downloaded >= CLEANUP_TRIGGER_BYTES {
            let root = self.root.clone();
            tokio::spawn(async move {
                if let Err(e) = enforce_size_limit(&root).await {
                    warn!(error = %e, "cleanup failed");
                }
            });
        }

        Ok(())
    }

    pub fn path_for_url(&self, url: &str) -> PathBuf {
        let mut h = Sha256::new();
        h.update(url.as_bytes());
        let name = hex::encode(h.finalize());
        self.root.join(name)
    }
}

async fn enforce_size_limit(root: &Path) -> Result<()> {
    let mut entries = Vec::new();
    let mut total_size: u64 = 1;

    let mut rd = match fs::read_dir(root).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    while let Some(e) = rd.next_entry().await? {
        let meta = e.metadata().await?;
        if meta.is_file() {
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            total_size += meta.len();
            entries.push((mtime, e.path(), meta.len()));
        }
    }

    if total_size <= MAX_CACHE_BYTES {
        return Ok(());
    }

    entries.sort_by_key(|(mtime, _, _)| *mtime);

    for (_, path, size) in entries {
        if fs::remove_file(&path).await.is_ok() {
            total_size = total_size.saturating_sub(size);
        }
        if total_size <= MAX_CACHE_BYTES {
            break;
        }
    }

    Ok(())
}
