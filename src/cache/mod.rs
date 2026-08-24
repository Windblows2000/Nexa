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
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{fs, io::AsyncWriteExt, sync::Mutex};
use tracing::{info, instrument, trace, warn};
use url::Url;
use uuid::Uuid;

const MAX_CACHE_BYTES: u64 = 1_000_000_000;
const MAX_OBJECT_BYTES: u64 = 10 * 1024 * 1024;
const CLEANUP_TRIGGER_BYTES: u64 = MAX_CACHE_BYTES / 100;
const SNIFF_BYTES: usize = 512;

#[derive(Clone)]
pub struct ImageCache {
    root: PathBuf,
    client: reqwest::Client,
    in_flight: Arc<Mutex<HashMap<String, InFlightEntry>>>,
}

struct InFlightEntry {
    lock: Arc<Mutex<()>>,
    users: usize,
}

impl ImageCache {
    pub async fn new() -> Result<Self> {
        let proj = ProjectDirs::from("com", "windblows2000", "nexa").context("cannot determine cache dir")?;
        let root = proj.cache_dir().join("art");
        fs::create_dir_all(&root).await?;

        Ok(Self {
            root,
            client: reqwest::Client::builder().timeout(Duration::from_secs(10)).user_agent("nexa/1.0").build()?,
            in_flight: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn resolve_data_uri(&self, data_uri: &str) -> Result<PathBuf> {
        let stem = self.stem_for_url(data_uri);

        if let Some(path) = self.cached_path(data_uri).await {
            trace!(path = ?path, "Base64 art already exists in cache");
            return Ok(path);
        }

        let Some(comma_pos) = data_uri.find(',') else {
            anyhow::bail!("Invalid Data URI");
        };

        let bytes = base64_decode(&data_uri[comma_pos + 1..]);

        let ext = infer::get(&bytes).map(|kind| kind.extension()).unwrap_or("bin");

        let final_path = self.root.join(format!("{stem}.{ext}"));
        fs::write(&final_path, bytes).await?;

        info!(path = ?final_path, "New Base64 art decoded and cached");
        Ok(final_path)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn stats(&self) -> Result<(usize, u64)> {
        let mut count = 0usize;
        let mut size = 0u64;

        let mut rd = match fs::read_dir(&self.root).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
            Err(e) => return Err(e.into()),
        };

        while let Some(entry) = rd.next_entry().await? {
            let meta = entry.metadata().await?;
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
        let stem = self.stem_for_url(url);

        for ext in ["jpg", "jpeg", "png", "webp", "gif", "bin"] {
            let path = self.root.join(format!("{stem}.{ext}"));
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

            if let Some(path) = self.cached_path(url).await {
                return Ok(path);
            }

            let stem = self.stem_for_url(url);
            self.download_to_cache(url, &stem).await
        }
        .await;

        self.release_in_flight(url, &lock).await;
        result
    }

    async fn acquire_in_flight(&self, url: &str) -> Arc<Mutex<()>> {
        let mut map = self.in_flight.lock().await;
        let entry = map
            .entry(url.to_owned())
            .or_insert_with(|| InFlightEntry { lock: Arc::new(Mutex::new(())), users: 0 });
        entry.users += 1;
        entry.lock.clone()
    }

    async fn release_in_flight(&self, url: &str, lock: &Arc<Mutex<()>>) {
        let mut map = self.in_flight.lock().await;

        let should_remove = if let Some(entry) = map.get_mut(url) {
            if Arc::ptr_eq(&entry.lock, lock) {
                entry.users = entry.users.saturating_sub(1);
                entry.users == 0
            } else {
                false
            }
        } else {
            false
        };

        if should_remove {
            map.remove(url);
        }
    }

    async fn is_valid_file(&self, path: &Path) -> bool {
        fs::metadata(path).await.map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
    }

    async fn download_to_cache(&self, url: &str, stem: &str) -> Result<PathBuf> {
        let parsed = Url::parse(url).context("invalid art URL")?;
        match parsed.scheme() {
            "http" | "https" => {}
            other => anyhow::bail!("unsupported art URL scheme: {other}"),
        }

        let resp = self.client.get(parsed).send().await?.error_for_status()?;
        if let Some(len) = resp.content_length()
            && len > MAX_OBJECT_BYTES
        {
            anyhow::bail!("object too large");
        }

        let tmp_path = self.root.join(format!("{stem}.tmp-{}", Uuid::new_v4()));
        let mut file = fs::File::create(&tmp_path).await?;
        let mut downloaded = 0u64;
        let mut sniff_buf = Vec::with_capacity(SNIFF_BYTES);
        let mut stream = resp.bytes_stream();

        let write_result: Result<()> = async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                downloaded += chunk.len() as u64;

                if downloaded > MAX_OBJECT_BYTES {
                    anyhow::bail!("exceeded max size");
                }

                if sniff_buf.len() < SNIFF_BYTES {
                    let remaining = SNIFF_BYTES - sniff_buf.len();
                    let take = remaining.min(chunk.len());
                    sniff_buf.extend_from_slice(&chunk[..take]);
                }

                file.write_all(&chunk).await?;
            }

            file.flush().await?;
            Ok(())
        }
        .await;

        if let Err(err) = write_result {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(err);
        }

        drop(file);

        let ext = infer::get(&sniff_buf).map(|kind| kind.extension()).unwrap_or("bin");
        let final_path = self.root.join(format!("{stem}.{ext}"));

        if let Err(err) = fs::rename(&tmp_path, &final_path).await {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(err.into());
        }

        info!(url = %url, path = ?final_path, "cached image");

        if downloaded >= CLEANUP_TRIGGER_BYTES {
            let root = self.root.clone();
            tokio::spawn(async move {
                if let Err(e) = enforce_size_limit(&root).await {
                    warn!(error = %e, "cleanup failed");
                }
            });
        }

        Ok(final_path)
    }

    fn stem_for_url(&self, url: &str) -> String {
        // 64-bit FNV-1a custom replacement for Sha256
        const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const PRIME: u64 = 0x100000001b3;

        let mut hash = OFFSET_BASIS;
        for &b in url.as_bytes() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(PRIME);
        }

        format!("{hash:016x}")
    }
}

fn base64_decode(input: &str) -> Vec<u8> {
    let mut output = Vec::new();
    let mut buffer = 0u32;
    let mut bits_collected = 0;

    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' => continue,
            _ => continue,
        };

        buffer = (buffer << 6) | (value as u32);
        bits_collected += 6;

        if bits_collected >= 8 {
            bits_collected -= 8;
            output.push((buffer >> bits_collected) as u8);
        }
    }
    output
}

async fn enforce_size_limit(root: &Path) -> Result<()> {
    let mut entries: Vec<(PathBuf, u64, Option<SystemTime>)> = Vec::new();
    let mut total_size = 0u64;

    let mut rd = match fs::read_dir(root).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    while let Some(entry) = rd.next_entry().await? {
        let meta = entry.metadata().await?;
        if meta.is_file() {
            total_size += meta.len();
            entries.push((entry.path(), meta.len(), meta.modified().ok()));
        }
    }

    if total_size <= MAX_CACHE_BYTES {
        return Ok(());
    }

    entries.sort_by_key(|(_, _, modified)| *modified);

    for (path, size, _) in entries {
        if fs::remove_file(&path).await.is_ok() {
            total_size = total_size.saturating_sub(size);
        }

        if total_size <= MAX_CACHE_BYTES {
            break;
        }
    }

    Ok(())
}
