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

use anyhow::Result;
use futures::SinkExt;
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::ipc::{Response, encode_response};

/// Send a single IPC response frame over a length-delimited framed unix stream.
pub async fn send(
    framed: &mut Framed<UnixStream, LengthDelimitedCodec>,
    resp: Response,
) -> Result<()> {
    framed.send(encode_response(&resp)?.into()).await?;
    Ok(())
}
