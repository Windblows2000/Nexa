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

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use tokio::sync::Mutex;

pub(crate) const MAX_CACHE_BYTES: u64 = 1_000_000_000;
pub(crate) const MAX_OBJECT_BYTES: u64 = 10 * 1024 * 1024;
pub(crate) const CLEANUP_TRIGGER_BYTES: u64 = MAX_CACHE_BYTES / 100;
pub(crate) const SNIFF_BYTES: usize = 512;

pub(crate) const CACHE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bin"];

#[derive(Clone)]
pub struct ImageCache {
    pub(crate) root: PathBuf,
    pub(crate) client: reqwest::Client,
    pub(crate) in_flight: Arc<Mutex<HashMap<String, InFlightEntry>>>,
}

pub(crate) struct InFlightEntry {
    pub(crate) lock: Arc<Mutex<()>>,
    pub(crate) users: usize,
}
