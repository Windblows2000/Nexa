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
    time::Duration,
};
use tokio::{fs, io::AsyncWriteExt, sync::Mutex};

const MAX_CACHE_BYTES: u64 = 1_000_000_000;
const MAX_OBJECT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone)]
pub struct ImageCache {
    root: PathBuf,
    client: reqwest::Client,
    in_flight: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
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
            in_flight: Arc::new(Mutex::new(HashMap::new())),
        })
    }
    pub async fn cached_path(&self, url: &str) -> Option<PathBuf> {
        let path = self.path_for_url(url);

        match fs::metadata(&path).await {
            Ok(meta) if meta.is_file() && meta.len() > 0 => Some(path),
            _ => None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

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

    pub async fn clear(&self) -> Result<()> {
        let _ = fs::remove_dir_all(&self.root).await;
        Ok(())
    }

    pub fn path_for_url(&self, url: &str) -> PathBuf {
        let mut h = Sha256::new();
        h.update(url.as_bytes());
        let name = hex::encode(h.finalize());
        self.root.join(name)
    }

    pub async fn ensure_cached(&self, url: &str) -> Result<PathBuf> {
        let path = self.path_for_url(url);

        if let Ok(meta) = fs::metadata(&path).await
            && meta.is_file()
            && meta.len() > 0
        {
            return Ok(path);
        }

        let url_lock = {
            let mut map = self.in_flight.lock().await;

            map.entry(url.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let _guard = url_lock.lock().await;

        if let Ok(meta) = fs::metadata(&path).await
            && meta.is_file()
            && meta.len() > 0
        {
            return Ok(path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let resp = self
        .client
        .get(url)
        .send()
        .await?
        .error_for_status()?;

        if let Some(len) = resp.content_length() {
            if len > MAX_OBJECT_BYTES {
                anyhow::bail!("album art too large: {} bytes", len);
            }
        }

        let tmp = path.with_extension("tmp");
        let mut file = fs::File::create(&tmp).await?;

        let mut downloaded: u64 = 0;
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            downloaded += chunk.len() as u64;

            if downloaded > MAX_OBJECT_BYTES {
                let _ = fs::remove_file(&tmp).await;
                anyhow::bail!("album art exceeded max size");
            }

            file.write_all(&chunk).await?;
        }

        file.flush().await?;
        drop(file);

        fs::rename(&tmp, &path).await?;

        if downloaded > MAX_CACHE_BYTES / 10 {
            let _ = enforce_size_limit(&self.root).await;
        }

        Ok(path)
    }
}

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
