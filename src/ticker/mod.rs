use tokio::sync::watch;
use tokio::time::{Duration, Instant, Interval, interval_at};
use tracing::{debug, trace};

use crate::daemon::state::DaemonState;

pub async fn run(state: DaemonState, mut enabled_rx: watch::Receiver<bool>) {
    let mut interval: Option<Interval> = None;
    let mut tick_count: u64 = 0;

    loop {
        tokio::select! {
            _ = async {
                if let Some(i) = &mut interval {
                    i.tick().await;
                } else {
                    futures::future::pending::<()>().await;
                }
            } => {
                tick_count += 1;
                trace!(tick_count, "ticker: tick fired; calling rebroadcast()");
                state.rebroadcast().await;
            }

            Ok(_) = enabled_rx.changed() => {
                let enabled = *enabled_rx.borrow();
                debug!(enabled, "ticker: enabled changed");

                if enabled {
                    interval = Some(second_aligned_interval());
                    debug!("ticker: interval enabled");
                } else {
                    interval = None;
                    debug!("ticker: interval disabled");
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
