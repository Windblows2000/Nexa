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
use mprizzle::Mpris;
use nexa::{
    cache::ImageCache,
    daemon::{server, state::DaemonState, supervisor},
    utils::init_logging,
};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    init_logging(args.verbose)?;

    let state = DaemonState::new();
    let mut mpris = Mpris::new().await?;
    let conn = mpris.connection();
    let cache = ImageCache::new()?;

    // Server can run independently (it only needs the shared DBus connection).
    let server_handle = tokio::spawn(server::run(state.clone(), conn.clone(), cache.clone()));

    // Supervisor owns the `Mpris` receiver loop (it needs `&mut Mpris`).
    supervisor::run(state.clone(), &mut mpris).await?;

    // If supervisor exits, stop the server too.
    server_handle.abort();

    Ok(())
}
