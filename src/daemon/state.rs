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

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::sync::{RwLock, broadcast};

use crate::{
    mpris::{PlayerStateSnapshot, PlayerStatus, TrackMetadata},
    player::{ActivityPriority, LivePlayer},
};

#[derive(Clone)]
pub struct DaemonState {
    inner: Arc<RwLock<Inner>>,
    tx: broadcast::Sender<PlayerStateSnapshot>,
}

pub type SharedState = DaemonState;

struct Inner {
    players: HashMap<String, LivePlayer>,
    primary_id: Option<String>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonState {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(RwLock::new(Inner {
                players: HashMap::new(),
                primary_id: None,
            })),
            tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PlayerStateSnapshot> {
        self.tx.subscribe()
    }

    pub async fn primary_snapshot(&self) -> Option<PlayerStateSnapshot> {
        let inner = self.inner.read().await;
        let id = inner.primary_id.as_ref()?;
        inner.players.get(id).map(|p| p.snapshot())
    }

    pub async fn snapshot_for_player(&self, player_id: &str) -> Option<PlayerStateSnapshot> {
        let inner = self.inner.read().await;
        inner.players.get(player_id).map(|p| p.snapshot())
    }

    pub async fn known_players(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.players.keys().cloned().collect()
    }

    pub async fn remove_player(&self, player_id: &str) {
        let mut inner = self.inner.write().await;
        if inner.players.remove(player_id).is_some()
            && inner.primary_id.as_deref() == Some(player_id)
        {
            inner.primary_id = pick_primary_id(&inner.players);
        }
    }

    pub fn should_tick(&self) -> bool {
        if self.tx.receiver_count() == 0 {
            return false;
        }

        let inner = self.inner.blocking_read();
        inner.primary_id.as_deref().is_some_and(|id| {
            inner
                .players
                .get(id)
                .is_some_and(|p| p.status == PlayerStatus::Playing)
        })
    }

    pub async fn rebroadcast(&self) {
        if self.tx.receiver_count() > 0
            && let Some(snapshot) = self.primary_snapshot().await
            && snapshot.status == PlayerStatus::Playing
        {
            let _ = self.tx.send(snapshot);
        }
    }

    pub async fn upsert_snapshot_and_broadcast(
        &self,
        snapshot: PlayerStateSnapshot,
        prio: ActivityPriority,
        should_broadcast: bool,
    ) {
        let id = snapshot.player_id.clone();
        let mut inner = self.inner.write().await;

        match inner.players.get_mut(&id) {
            Some(p) => p.apply_snapshot(snapshot, prio),
            None => {
                inner.players.insert(id.clone(), LivePlayer::new(snapshot));
            }
        }

        inner.primary_id = pick_primary_id(&inner.players);
        self.finalize_update(inner, &id, should_broadcast);
    }

    pub async fn apply_update_id_selective(
        &self,
        player_id: &str,
        update: PlayerUpdate,
        prio: ActivityPriority,
        should_broadcast: bool,
    ) {
        let mut inner = self.inner.write().await;

        let status_changed = if let Some(p) = inner.players.get_mut(player_id) {
            let old_status = p.status;
            update.apply(p, prio);
            old_status != p.status
        } else {
            return;
        };

        if status_changed || inner.primary_id.is_none() {
            inner.primary_id = pick_primary_id(&inner.players);
        }

        self.finalize_update(inner, player_id, should_broadcast);
    }

    fn finalize_update(
        &self,
        inner: tokio::sync::RwLockWriteGuard<'_, Inner>,
        id: &str,
        should_broadcast: bool,
    ) {
        let snap = if should_broadcast && self.tx.receiver_count() > 0 {
            inner.players.get(id).map(|p| p.snapshot())
        } else {
            None
        };

        drop(inner);

        if let Some(s) = snap {
            let _ = self.tx.send(s);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlayerUpdate {
    pub status: Option<PlayerStatus>,
    pub metadata: Option<TrackMetadata>,
    pub rate: Option<f64>,
    pub volume: Option<f64>,
    pub shuffle: Option<bool>,
    pub loop_status: Option<String>,
    pub position_micros: Option<i64>,
}

impl PlayerUpdate {
    fn apply(self, p: &mut LivePlayer, prio: ActivityPriority) {
        if let Some(s) = self.status {
            p.apply_status(s);
        }
        if let Some(m) = self.metadata {
            p.apply_metadata(m);
        }
        if let Some(r) = self.rate {
            p.rate = Some(r);
        }
        if let Some(v) = self.volume {
            p.volume = Some(v);
        }
        if let Some(sh) = self.shuffle {
            p.shuffle = Some(sh);
        }
        if let Some(ls) = self.loop_status {
            p.loop_status = Some(ls);
        }
        if let Some(micros) = self.position_micros.filter(|&m| m >= 0) {
            p.apply_position_update(Duration::from_micros(micros as u64));
        }
        p.last_activity_priority = p.last_activity_priority.max(prio);
    }
}

fn pick_primary_id(players: &HashMap<String, LivePlayer>) -> Option<String> {
    players
        .iter()
        .max_by(|(_, a), (_, b)| {
            match (
                a.status == PlayerStatus::Playing,
                b.status == PlayerStatus::Playing,
            ) {
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ => a
                    .last_activity_priority
                    .cmp(&b.last_activity_priority)
                    .then_with(|| a.last_activity.cmp(&b.last_activity)),
            }
        })
        .map(|(id, _)| id.clone())
}

#[derive(Default)]
pub struct FollowState {
    last_snapshot: Option<PlayerStateSnapshot>,
}

impl FollowState {
    pub fn should_emit(&mut self, next: &PlayerStateSnapshot) -> bool {
        let Some(prev) = &self.last_snapshot else {
            self.last_snapshot = Some(next.clone());
            return true;
        };

        let same_meta = prev.player_id == next.player_id
            && prev.status == next.status
            && prev.metadata == next.metadata
            && same_f64(prev.rate, next.rate)
            && same_f64(prev.volume, next.volume)
            && prev.shuffle == next.shuffle
            && prev.loop_status == next.loop_status;

        if same_meta
            && (prev.position.as_secs() == next.position.as_secs()
                || next.status != PlayerStatus::Playing)
        {
            return false;
        }

        self.last_snapshot = Some(next.clone());
        true
    }
}

fn same_f64(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() <= f64::EPSILON,
        (None, None) => true,
        _ => false,
    }
}
