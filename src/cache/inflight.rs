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

use std::sync::Arc;

use tokio::sync::Mutex;

use super::types::{ImageCache, InFlightEntry};

impl ImageCache {
    pub(crate) async fn acquire_in_flight(&self, url: &str) -> Arc<Mutex<()>> {
        let mut map = self.in_flight.lock().await;

        let entry = map.entry(url.to_owned()).or_insert_with(|| InFlightEntry { lock: Arc::new(Mutex::new(())), users: 0 });

        entry.users += 1;
        Arc::clone(&entry.lock)
    }

    pub(crate) async fn release_in_flight(&self, url: &str, lock: &Arc<Mutex<()>>) {
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
}
