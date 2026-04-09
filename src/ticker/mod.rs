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

use crate::daemon::state::DaemonState;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tokio::time::{Duration, Instant, Interval, interval_at};
use tracing::{debug, trace};

pub async fn run(state: DaemonState, mut demand_rx: watch::Receiver<usize>) {
    let mut interval: Option<Interval> = None;

    loop {
        tokio::select! {
            _ = async {
                if let Some(interval) = &mut interval {
                    interval.tick().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                if state.should_tick() {
                    trace!("ticker: tick fired; rebroadcasting");
                    state.rebroadcast().await;
                }
            }
            Ok(_) = demand_rx.changed() => {
                let demand = *demand_rx.borrow();
                trace!(demand, "ticker: demand changed");

                match (demand > 0, interval.is_some()) {
                    (true, false) => {
                        interval = Some(second_aligned_interval());
                        debug!("ticker: enabled");
                    }
                    (false, true) => {
                        interval = None;
                        debug!("ticker: disabled");
                    }
                    _ => {}
                }
            }
        }
    }
}

fn second_aligned_interval() -> Interval {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let nanos = now.subsec_nanos() as u64;
    let delay = if nanos == 0 {
        Duration::from_secs(1)
    } else {
        Duration::from_nanos(1_000_000_000 - nanos)
    };

    interval_at(Instant::now() + delay, Duration::from_secs(1))
}
