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
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::{RwLock, broadcast};
use tracing::{debug, trace, warn};

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
        trace!("Initializing new DaemonState");
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
        trace!(
            receivers = self.tx.receiver_count(),
            "New subscriber added to broadcast channel"
        );
        self.tx.subscribe()
    }

    pub async fn primary_snapshot(&self) -> Option<PlayerStateSnapshot> {
        let inner = self.inner.read().await;
        let id = inner.primary_id.as_ref()?;
        let snap = inner.players.get(id).map(|p| p.snapshot());
        trace!(
            player_id = ?id,
            success = snap.is_some(),
               "Primary snapshot requested"
        );
        snap
    }

    pub async fn snapshot_for_player(&self, player_id: &str) -> Option<PlayerStateSnapshot> {
        let inner = self.inner.read().await;
        let snap = inner.players.get(player_id).map(|p| p.snapshot());
        trace!(
            player_id,
            success = snap.is_some(),
            "Player snapshot requested"
        );
        snap
    }

    pub async fn known_players(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        let keys: Vec<String> = inner.players.keys().cloned().collect();
        trace!(count = keys.len(), "Known players list requested");
        keys
    }

    pub async fn remove_player(&self, player_id: &str) {
        let mut inner = self.inner.write().await;
        let existed = inner.players.remove(player_id).is_some();
        trace!(player_id, existed, "Removing player from state");

        if !existed {
            warn!(player_id, "Attempted to remove non-existent player");
        }

        if inner.primary_id.as_deref() == Some(player_id) {
            let old_primary = inner.primary_id.clone();
            inner.primary_id = pick_primary_id(&inner.players);
            debug!(
                old_primary = ?old_primary,
                new_primary = ?inner.primary_id,
                "Primary player reassigned after removal"
            );
        }
    }

    pub fn should_tick(&self) -> bool {
        if self.tx.receiver_count() == 0 {
            trace!("should_tick: no receivers; false");
            return false;
        }

        let inner = self.inner.blocking_read();
        let Some(id) = inner.primary_id.as_deref() else {
            trace!("should_tick: no primary; false");
            return false;
        };

        let playing = inner
            .players
            .get(id)
            .is_some_and(|p| p.status == PlayerStatus::Playing);

        trace!(player_id = %id, playing, "should_tick evaluated");
        playing
    }

    pub async fn rebroadcast(&self) {
        if self.tx.receiver_count() == 0 {
            trace!("rebroadcast: no receivers; skipping");
            return;
        }

        let Some(snapshot) = self.primary_snapshot().await else {
            trace!("rebroadcast: no primary snapshot; skipping");
            return;
        };

        trace!(
            player_id = %snapshot.player_id,
            status = ?snapshot.status,
            position_us = snapshot.position.as_micros(),
               receivers = self.tx.receiver_count(),
               "rebroadcast: primary snapshot fetched"
        );

        if snapshot.status != PlayerStatus::Playing {
            trace!("rebroadcast: primary not playing; skipping send");
            return;
        }

        match self.tx.send(snapshot) {
            Ok(sent) => trace!(sent, "rebroadcast: broadcast sent"),
            Err(e) => warn!(error = ?e, "rebroadcast: broadcast send failed"),
        }
    }

    pub async fn upsert_snapshot_and_broadcast(
        &self,
        snapshot: PlayerStateSnapshot,
        prio: ActivityPriority,
        should_broadcast: bool,
    ) {
        let id = snapshot.player_id.clone();
        trace!(
            player_id = %id,
            prio = ?prio,
            should_broadcast,
            receivers = self.tx.receiver_count(),
               "Upsert snapshot requested"
        );

        let mut inner = self.inner.write().await;
        let old_primary = inner.primary_id.clone();

        match inner.players.get_mut(&id) {
            Some(p) => {
                trace!(player_id = %id, "Applying snapshot update to existing player");
                p.apply_snapshot(snapshot, prio);
            }
            None => {
                debug!(player_id = %id, "Inserting new player into state");
                inner.players.insert(id.clone(), LivePlayer::new(snapshot));
            }
        }

        inner.primary_id = pick_primary_id(&inner.players);

        trace!(
            old_primary = ?old_primary,
            new_primary = ?inner.primary_id,
            "Primary player evaluation complete"
        );

        let receiver_count = self.tx.receiver_count();
        let snap = if should_broadcast && receiver_count > 0 {
            inner.players.get(&id).map(|p| p.snapshot())
        } else {
            None
        };

        drop(inner);

        match snap {
            Some(snap) => {
                trace!(
                    player_id = %id,
                    receivers = receiver_count,
                    status = ?snap.status,
                    "Broadcasting player snapshot"
                );
                let _ = self.tx.send(snap);
            }
            None => {
                trace!(
                    player_id = %id,
                    receivers = receiver_count,
                    "Snapshot suppressed"
                );
            }
        }
    }

    pub async fn apply_update_id_selective(
        &self,
        player_id: &str,
        update: PlayerUpdate,
        prio: ActivityPriority,
        should_broadcast: bool,
    ) {
        trace!(
            player_id,
            has_status = update.status.is_some(),
               has_metadata = update.metadata.is_some(),
               has_rate = update.rate.is_some(),
               has_volume = update.volume.is_some(),
               has_shuffle = update.shuffle.is_some(),
               has_loop = update.loop_status.is_some(),
               has_position = update.position_micros.is_some(),
               prio = ?prio,
               should_broadcast,
               receivers = self.tx.receiver_count(),
               "Selective update received"
        );

        let mut inner = self.inner.write().await;

        let is_currently_primary = inner.primary_id.as_deref() == Some(player_id);

        let (status_changed, old_status, new_status) =
            if let Some(p) = inner.players.get_mut(player_id) {
                let old_status = p.status;
                trace!(player_id, "Applying selective update to player");
                update.apply(p, prio);
                let new_status = p.status;
                (old_status != new_status, old_status, new_status)
            } else {
                warn!(player_id, "Received update for unknown player; ignoring");
                return;
            };

        trace!(
            player_id,
            status_changed,
            old_status = ?old_status,
            new_status = ?new_status,
            is_currently_primary,
            "Selective update applied"
        );

        let old_primary = inner.primary_id.clone();

        if status_changed || !is_currently_primary {
            inner.primary_id = pick_primary_id(&inner.players);
            trace!(
                old_primary = ?old_primary,
                new_primary = ?inner.primary_id,
                "Primary player evaluation complete"
            );
        } else {
            trace!(player_id, "Skipping re-election: player is already primary");
        }

        let receiver_count = self.tx.receiver_count();
        let snap = if should_broadcast && receiver_count > 0 {
            inner.players.get(player_id).map(|p| p.snapshot())
        } else {
            None
        };

        drop(inner);

        match snap {
            Some(snap) => {
                trace!(
                    player_id,
                    receivers = receiver_count,
                    status = ?snap.status,
                    position_us = snap.position.as_micros(),
                       "Broadcasting selective update snapshot"
                );
                let _ = self.tx.send(snap);
            }
            None => {
                trace!(
                    player_id,
                    receivers = receiver_count,
                    "Selective update snapshot suppressed"
                );
            }
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
        trace!(
            player_id = %p.player_id,
            status = self.status.is_some(),
               metadata = self.metadata.is_some(),
               rate = self.rate.is_some(),
               volume = self.volume.is_some(),
               shuffle = self.shuffle.is_some(),
               loop_status = self.loop_status.is_some(),
               position = self.position_micros,
               prio = ?prio,
               "Applying PlayerUpdate fields"
        );

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
        if let Some(micros) = self.position_micros
            && micros >= 0
        {
            p.apply_position_update(Duration::from_micros(micros as u64));
        }

        p.last_activity_priority = p.last_activity_priority.max(prio);

        trace!(
            player_id = %p.player_id,
            status = ?p.status,
            last_activity = ?p.last_activity,
            last_priority = ?p.last_activity_priority,
            "Player activity updated"
        );
    }
}

fn pick_primary_id(players: &HashMap<String, LivePlayer>) -> Option<String> {
    let chosen = players
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
        .map(|(id, _)| id.clone());

    trace!(primary_id = ?chosen, count = players.len(), "Primary player election completed");
    chosen
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
            if !position_changed || next.status != PlayerStatus::Playing {
                return false;
            }
        }

        trace!(
            player_id = %next.player_id,
            status = ?next.status,
            "FollowState emitting snapshot"
        );

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
