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

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::Result;
use tokio::fs;

use super::{
    index::CACHE_INDEX_FILE,
    types::{ImageCache, MAX_CACHE_BYTES},
};

impl ImageCache {
    pub async fn stats(&self) -> Result<(usize, u64)> {
        let mut count = 0usize;
        let mut size = 0u64;

        let mut entries = match fs::read_dir(&self.root).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok((0, 0));
            }
            Err(error) => return Err(error.into()),
        };

        while let Some(entry) = entries.next_entry().await? {
            if is_internal_cache_file(&entry.path()) {
                continue;
            }

            let metadata = entry.metadata().await?;

            if metadata.is_file() {
                count += 1;
                size += metadata.len();
            }
        }

        Ok((count, size))
    }

    pub async fn clear(&self) -> Result<()> {
        match fs::remove_dir_all(&self.root).await {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        fs::create_dir_all(&self.root).await?;

        Ok(())
    }
}

pub(crate) async fn enforce_size_limit(root: &Path) -> Result<()> {
    let mut entries: Vec<CacheFile> = Vec::new();
    let mut total_size = 0u64;

    let mut directory = match fs::read_dir(root).await {
        Ok(directory) => directory,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };

    while let Some(entry) = directory.next_entry().await? {
        let path = entry.path();

        if is_internal_cache_file(&path) {
            continue;
        }

        let metadata = entry.metadata().await?;

        if metadata.is_file() {
            total_size += metadata.len();

            entries.push(CacheFile { path, size: metadata.len(), modified: metadata.modified().ok() });
        }
    }

    if total_size <= MAX_CACHE_BYTES {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.modified);

    for entry in entries {
        if fs::remove_file(&entry.path).await.is_ok() {
            total_size = total_size.saturating_sub(entry.size);
        }

        if total_size <= MAX_CACHE_BYTES {
            break;
        }
    }

    Ok(())
}

struct CacheFile {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

fn is_internal_cache_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == CACHE_INDEX_FILE || name.starts_with(&format!("{CACHE_INDEX_FILE}.tmp-")))
}
