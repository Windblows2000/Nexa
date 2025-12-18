// Copyright (C) 2025 Windblows2000
// This file is part of rusty-player.
//
// rusty-player is free software: you can redistribute it and/or modify
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
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use mprizzle::PlayerIdentity;
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
    players: HashMap<PlayerIdentity, LivePlayer>,
    primary_id: Option<PlayerIdentity>,
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

    fn id_from_bus(bus: &str) -> Option<PlayerIdentity> {
        PlayerIdentity::new(bus.to_string()).ok()
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
        let id = Self::id_from_bus(player_id)?;
        let inner = self.inner.read().await;
        inner.players.get(&id).map(|p| p.snapshot())
    }

    pub async fn known_players(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner
            .players
            .keys()
            .map(|id| id.bus().to_string())
            .collect()
    }

    pub async fn remove_player(&self, player_id: &str) {
        let Some(id) = Self::id_from_bus(player_id) else {
            return;
        };

        let mut inner = self.inner.write().await;
        inner.players.remove(&id);

        if inner.primary_id.as_ref() == Some(&id) {
            inner.primary_id = pick_primary_id(&inner.players);
        }
    }

    pub async fn remove_player_id(&self, identity: &PlayerIdentity) {
        let mut inner = self.inner.write().await;
        inner.players.remove(identity);

        if inner.primary_id.as_ref() == Some(identity) {
            inner.primary_id = pick_primary_id(&inner.players);
        }
    }

    pub async fn upsert_snapshot(&self, snapshot: PlayerStateSnapshot, prio: ActivityPriority) {
        let Some(id) = PlayerIdentity::new(snapshot.player_id.clone()).ok() else {
            return;
        };

        let mut inner = self.inner.write().await;

        match inner.players.get_mut(&id) {
            Some(p) => p.apply_snapshot(snapshot, prio),
            None => {
                inner.players.insert(id.clone(), LivePlayer::new(snapshot));
            }
        }

        inner.primary_id = pick_primary_id(&inner.players);
        let snap = inner.players.get(&id).map(|p| p.snapshot());
        drop(inner);

        if let Some(snap) = snap {
            let _ = self.tx.send(snap);
        }
    }

    pub async fn apply_update_id(
        &self,
        identity: &PlayerIdentity,
        update: PlayerUpdate,
        prio: ActivityPriority,
    ) {
        let mut inner = self.inner.write().await;

        // 1. Apply update in its own scope
        {
            let Some(p) = inner.players.get_mut(identity) else {
                return;
            };
            update.apply(p, prio);
        } // <- mutable borrow of `p` ends here

        // 2. Now we can safely touch `inner` again
        inner.primary_id = pick_primary_id(&inner.players);

        // 3. Snapshot immutably
        let snap = inner.players.get(identity).map(|p| p.snapshot());
        drop(inner);

        if let Some(snap) = snap {
            let _ = self.tx.send(snap);
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
        let now = Instant::now();

        if let Some(s) = self.status {
            p.status = s;
        }
        if let Some(m) = self.metadata {
            p.metadata = m;
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

        if let Some(micros) = self.position_micros
            && micros >= 0
        {
            p.reanchor_position(now, Duration::from_micros(micros as u64));
        }

        p.last_activity = now;
        p.last_activity_priority = p.last_activity_priority.max(prio);
    }
}

fn pick_primary_id(players: &HashMap<PlayerIdentity, LivePlayer>) -> Option<PlayerIdentity> {
    players
        .iter()
        .max_by(|(_, a), (_, b)| {
            let a_playing = a.status == PlayerStatus::Playing;
            let b_playing = b.status == PlayerStatus::Playing;

            match (a_playing, b_playing) {
                (true, false) => return std::cmp::Ordering::Greater,
                (false, true) => return std::cmp::Ordering::Less,
                _ => {}
            }

            a.last_activity_priority
                .cmp(&b.last_activity_priority)
                .then_with(|| a.last_activity.cmp(&b.last_activity))
        })
        .map(|(id, _)| id.clone())
}

#[derive(Default)]
pub struct FollowState {
    last_snapshot: Option<PlayerStateSnapshot>,
    last_emit: Option<Instant>,
}

impl FollowState {
    pub fn should_emit(&mut self, next: &PlayerStateSnapshot) -> bool {
        let now = Instant::now();

        let Some(prev) = &self.last_snapshot else {
            self.last_snapshot = Some(next.clone());
            self.last_emit = Some(now);
            return true;
        };

        let same_non_position = prev.player_id == next.player_id
            && prev.status == next.status
            && prev.metadata == next.metadata
            && same_f64(prev.rate, next.rate)
            && same_f64(prev.volume, next.volume)
            && prev.shuffle == next.shuffle
            && prev.loop_status == next.loop_status;

        if same_non_position {
            let position_changed = prev.position.as_secs() != next.position.as_secs();
            if position_changed && next.status == PlayerStatus::Playing {
                if let Some(last) = self.last_emit
                    && now.duration_since(last) < Duration::from_secs(1)
                {
                    return false;
                }
            } else {
                return false;
            }
        }

        self.last_snapshot = Some(next.clone());
        self.last_emit = Some(now);
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
