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

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use futures_util::{SinkExt, StreamExt};
use nexa::cli::{Cli, Cmd};
use nexa::ipc::{PlayerSnapshotOut, Response, decode_response, encode_request, socket_path};
use nexa::utils::init_logging;
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose)?;

    if let Cmd::Completions { shell } = &cli.cmd {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        generate(*shell, &mut cmd, name, &mut std::io::stdout());
        return Ok(());
    }

    if let Cmd::Cache { cmd } = &cli.cmd {
        nexa::cli::handle_cache(cmd.clone()).await?;
        return Ok(());
    }

    let req = nexa::cli::to_request(&cli)?.expect("non-local commands must produce a request");

    let stream = UnixStream::connect(socket_path()).await.context("daemon not running (start `nexad`)")?;
    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

    framed.send(encode_request(&req)?.into()).await?;

    let mut last_snapshot: Option<PlayerSnapshotOut> = None;

    while let Some(bytes) = framed.next().await.transpose()? {
        let resp = decode_response(&bytes)?;

        match resp {
            Response::Metadata(snap) => {
                let snap = *snap;
                last_snapshot = Some(snap.clone());

                match &cli.cmd {
                    Cmd::Metadata { out, format, .. } | Cmd::Follow { out, format, .. } => {
                        out.print(&snap, format.as_deref())?;
                    }
                    Cmd::Status { out, .. } | Cmd::List { out, .. } => {
                        out.print(&snap, None)?;
                    }
                    _ => {}
                }
            }
            Response::Position(seconds) => {
                if let Some(mut snap) = last_snapshot.clone() {
                    snap.elapsed = seconds;
                    if let Cmd::Follow { out, format, .. } = &cli.cmd {
                        out.print(&snap, format.as_deref())?;
                    }
                    last_snapshot = Some(snap);
                }
            }
            Response::Status(status) => println!("{status}"),
            Response::List(players) => {
                for player in players {
                    println!("{player}");
                }
            }
            Response::Ok(msg) => match msg {
                Some(text) if text.parse::<f64>().is_ok() || text.contains(':') => {
                    println!("{text}");
                }
                Some(text) => println!("changed state to {text}"),
                None => println!("ok"),
            },
            Response::Pong => {}
            Response::Error(err) => anyhow::bail!(err),
        }

        if !matches!(cli.cmd, Cmd::Follow { .. }) {
            break;
        }
    }

    Ok(())
}
