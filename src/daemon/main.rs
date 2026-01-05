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

use anyhow::Result;
use clap::Parser;
use nexa::ticker;
use tokio::sync::watch;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use zbus::Connection;

use nexa::{
    cache::ImageCache,
    daemon::{server, state::DaemonState, supervisor},
};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();

    let (ticker_tx, ticker_rx) = watch::channel::<usize>(0);

    let level = match args.verbose {
        0 => LevelFilter::INFO,
        1 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(level.into())
                .from_env_lossy()
                .add_directive("zbus=off".parse().unwrap())
                .add_directive("zbus_names=off".parse().unwrap())
                .add_directive("zvariant=off".parse().unwrap()),
        )
        .init();

    tracing::info!(%level, "starting nexa daemon");

    let state = DaemonState::new();
    let cache = ImageCache::new().await?;

    let conn = Connection::session().await?;

    let supervisor_task = {
        let state = state.clone();
        let conn = conn.clone();
        tokio::spawn(async move {
            if let Err(e) = supervisor::run(state, conn).await {
                tracing::error!(error = %e, "supervisor crashed");
            }
        })
    };

    let _ticker_task = {
        let state = state.clone();
        tokio::spawn(async move {
            ticker::run(state, ticker_rx).await;
        })
    };

    let server_task = {
        let state = state.clone();
        let conn = conn.clone();
        let ticker_tx = ticker_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = server::run(state, conn, cache, ticker_tx).await {
                tracing::error!(error = %e, "ipc server crashed");
            }
        })
    };

    tokio::select! {
        _ = supervisor_task => {
            tracing::warn!("supervisor task exited");
        }
        _ = server_task => {
            tracing::warn!("server task exited");
        }
    }

    Ok(())
}
