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

use tokio::sync::watch;
use tokio::time::{Duration, Instant, Interval, interval_at};
use tracing::{debug, trace};

use crate::daemon::state::DaemonState;

pub async fn run(state: DaemonState, mut demand_rx: watch::Receiver<usize>) {
    let mut interval: Option<Interval> = None;

    loop {
        tokio::select! {
            _ = async {
                if let Some(i) = &mut interval {
                    i.tick().await;
                } else {
                    futures::future::pending::<()>().await;
                }
            } => {
                trace!("ticker: tick fired; rebroadcasting");
                state.rebroadcast().await;
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
    let now = Instant::now();
    let next = now + Duration::from_secs(1)
        - Duration::from_nanos((now.elapsed().as_nanos() % 1_000_000_000) as u64);

    interval_at(next, Duration::from_secs(1))
}
