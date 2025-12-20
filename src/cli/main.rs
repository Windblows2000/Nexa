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
use clap::Parser;
use futures::{SinkExt, StreamExt};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use nexa::cli::{Cli, Cmd};
use nexa::ipc::{Response, decode_response, encode_request, socket_path};
use nexa::utils::init_logging;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose)?;

    // ---- client-side commands (no daemon needed) ----
    if let Cmd::Cache { cmd } = &cli.cmd {
        nexa::cli::handle_cache(cmd.clone()).await?;
        return Ok(());
    }

    // ---- build IPC request ----
    let req = nexa::cli::to_request(&cli)?.expect("non-cache commands must produce a request");

    let stream = UnixStream::connect(socket_path())
        .await
        .context("daemon not running (start `nexad`)")?;

    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());
    framed.send(encode_request(&req)?.into()).await?;

    // ---- output selection ----
    let (template, toml) = match &cli.cmd {
        Cmd::Metadata { out, .. } | Cmd::Follow { out, .. } => (out.format.as_deref(), out.toml),

        Cmd::Status { out, .. } | Cmd::List { out, .. } => (None, out.toml),

        _ => (None, false),
    };

    // ---- response loop ----
    while let Some(bytes) = framed.next().await.transpose()? {
        let resp = decode_response(&bytes)?;

        match resp {
            Response::Metadata(snap) => {
                nexa::cli::print_metadata(&snap, template, toml)?;
            }

            Response::Status(s) => {
                println!("{s}");
            }

            Response::List(players) => {
                for p in players {
                    println!("{p}");
                }
            }

            Response::Ok => {
                println!("ok");
            }

            Response::Pong => {
                // silent by default
            }

            Response::Error(e) => {
                anyhow::bail!(e);
            }
        }

        // Non-follow commands exit after first response
        if !matches!(cli.cmd, Cmd::Follow { .. }) {
            break;
        }
    }

    Ok(())
}
