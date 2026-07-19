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

use std::time::{Duration, Instant};
use tracing::{debug, info};

use crate::mpris::{PlayerStateSnapshot, PlayerStatus, TrackMetadata};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ActivityPriority {
    Disconnected = 0,
    PositionUpdate = 1,
    MetadataUpdate = 2,
    StatusUpdate = 3,
}

#[derive(Debug, Clone)]
pub struct LivePlayer {
    pub player_id: String,
    pub identity: String,
    pub status: PlayerStatus,
    pub metadata: TrackMetadata,
    pub rate: Option<f64>,
    pub volume: Option<f64>,
    pub shuffle: Option<bool>,
    pub loop_status: Option<String>,

    pub position_anchor: Duration,
    pub anchor_timestamp: Instant,

    pub last_activity: Instant,
    pub last_activity_priority: ActivityPriority,
}

impl LivePlayer {
    pub fn new(snapshot: PlayerStateSnapshot) -> Self {
        let now = Instant::now();
        Self {
            player_id: snapshot.player_id,
            identity: snapshot.identity,
            status: snapshot.status,
            metadata: snapshot.metadata,
            rate: snapshot.rate,
            volume: snapshot.volume,
            shuffle: snapshot.shuffle,
            loop_status: snapshot.loop_status,
            position_anchor: snapshot.position,
            anchor_timestamp: now,
            last_activity: now,
            last_activity_priority: ActivityPriority::MetadataUpdate,
        }
    }

    pub fn position(&self) -> Duration {
        match self.status {
            PlayerStatus::Playing => {
                let elapsed = Instant::now().saturating_duration_since(self.anchor_timestamp);
                let rate = self.rate.unwrap_or(1.0).max(0.0);
                self.position_anchor + elapsed.mul_f64(rate)
            }
            _ => self.position_anchor,
        }
    }

    fn freeze_position(&mut self, now: Instant) {
        self.position_anchor = self.position();
        self.anchor_timestamp = now;
    }

    pub fn reanchor_position(&mut self, now: Instant, pos: Duration) {
        self.position_anchor = pos;
        self.anchor_timestamp = now;
    }

    pub fn apply_snapshot(&mut self, mut snapshot: PlayerStateSnapshot, prio: ActivityPriority) {
        let now = Instant::now();

        if snapshot.metadata.track_id.is_none() {
            snapshot.metadata.track_id = self.metadata.track_id.clone();
        }

        let track_changed = snapshot.metadata.track_id != self.metadata.track_id
            || snapshot.metadata.title != self.metadata.title
            || snapshot.metadata.artist != self.metadata.artist;

        let cur_est = self.position();
        let drift = snapshot.position.abs_diff(cur_est);

        let position_jumped = drift > Duration::from_millis(1200);

        if self.status == PlayerStatus::Playing && snapshot.status != PlayerStatus::Playing {
            self.freeze_position(now);
        }

        self.status = snapshot.status;
        self.metadata = snapshot.metadata;
        self.rate = snapshot.rate;
        self.volume = snapshot.volume;
        self.shuffle = snapshot.shuffle;
        self.loop_status = snapshot.loop_status;

        if track_changed {
            info!(id = %self.player_id, pos = ?snapshot.position, "Track changed, re-anchoring");
            self.reanchor_position(now, snapshot.position);
        } else if position_jumped {
            debug!(id = %self.player_id, drift = ?drift, "Seek detected, re-anchoring");
            self.reanchor_position(now, snapshot.position);
        }

        self.last_activity = now;
        self.last_activity_priority = prio;
    }

    pub fn apply_status(&mut self, new_status: PlayerStatus) {
        if self.status == new_status {
            return;
        }

        let now = Instant::now();
        match (self.status, new_status) {
            (PlayerStatus::Playing, _) => self.freeze_position(now),
            (_, PlayerStatus::Playing) => self.anchor_timestamp = now,
            _ => {}
        }

        debug!(id = %self.player_id, from = ?self.status, to = ?new_status, "Status transition");
        self.status = new_status;
        self.last_activity = now;
        self.last_activity_priority = ActivityPriority::StatusUpdate;
    }

    pub fn apply_metadata(&mut self, mut md: TrackMetadata) {
        let now = Instant::now();
        if md.track_id.is_none() {
            md.track_id = self.metadata.track_id.clone();
        }

        if md.track_id != self.metadata.track_id {
            info!(id = %self.player_id, "Metadata track_id changed");
            self.reanchor_position(now, Duration::ZERO);
        }

        self.metadata = md;
        self.last_activity = now;
        self.last_activity_priority = ActivityPriority::MetadataUpdate;
    }

    pub fn apply_position_update(&mut self, position: Duration) {
        let now = Instant::now();
        let cur_est = self.position();

        let drift = position.abs_diff(cur_est);
        if drift < Duration::from_millis(150) {
            return;
        }

        debug!(id = %self.player_id, position = ?position, drift = ?drift, "Position sync");
        self.reanchor_position(now, position);
        self.last_activity = now;
        self.last_activity_priority = self
            .last_activity_priority
            .max(ActivityPriority::PositionUpdate);
    }

    pub fn snapshot(&self) -> PlayerStateSnapshot {
        PlayerStateSnapshot {
            player_id: self.player_id.clone(),
            identity: self.identity.clone(),
            status: self.status,
            metadata: self.metadata.clone(),
            position: self.position(),
            rate: self.rate,
            volume: self.volume,
            shuffle: self.shuffle,
            loop_status: self.loop_status.clone(),
        }
    }
}
