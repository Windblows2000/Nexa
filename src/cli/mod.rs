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
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::cache::ImageCache;
use crate::ipc::{Command, LoopState, PlayerSnapshotOut, Request, ShuffleState, Target};

//
// ===== Output =====
//

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Text,
    Json,
    Toml,
}

#[derive(Args, Debug, Clone, Copy)]
pub struct OutputArgs {
    /// Force TOML output
    #[arg(long, conflicts_with = "output")]
    pub toml: bool,

    /// Explicit output mode
    #[arg(long, value_enum)]
    pub output: Option<OutputMode>,
}

impl OutputArgs {
    pub fn resolve(&self, default: OutputMode, has_format: bool) -> OutputMode {
        if self.toml {
            OutputMode::Toml
        } else if has_format {
            OutputMode::Text
        } else {
            self.output.unwrap_or(default)
        }
    }
}

//
// ===== CLI =====
//

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Cli {
    /// Increase logging verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub cmd: Cmd,
}

//
// ===== Commands =====
//

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List detected MPRIS players.
    List {
        #[arg(long)]
        filter: Option<String>,

        #[command(flatten)]
        out: OutputArgs,
    },

    /// Show current playback status.
    Status {
        #[arg(long, default_value = "best")]
        player: String,

        #[command(flatten)]
        out: OutputArgs,
    },

    /// Print current metadata once.
    Metadata {
        #[arg(long, default_value = "best")]
        player: String,

        /// Template for text output
        #[arg(long)]
        format: Option<String>,

        #[command(flatten)]
        out: OutputArgs,
    },

    /// Stream metadata updates.
    Follow {
        #[arg(long, default_value = "best")]
        player: String,

        /// Template for text output
        #[arg(long)]
        format: Option<String>,

        #[command(flatten)]
        out: OutputArgs,
    },

    /// Send a playback control command.
    Command {
        #[arg(long, default_value = "best")]
        player: String,

        #[command(subcommand)]
        cmd: ControlCmd,
    },

    /// Inspect or manage the album art cache.
    Cache {
        #[command(subcommand)]
        cmd: CacheCmd,
    },

    /// Ping the daemon.
    Ping,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CacheCmd {
    /// Show cache size and location.
    Info,

    /// Remove all cached album art.
    Clean,
}

//
// ===== Control Commands =====
//

#[derive(Subcommand, Debug, Clone)]
pub enum ControlCmd {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,

    Open {
        uri: String,
    },

    Volume {
        #[arg(long)]
        set: Option<f64>,
        #[arg(long)]
        up: Option<f64>,
        #[arg(long)]
        down: Option<f64>,
    },

    Position {
        #[arg(long)]
        set: Option<u64>,
        #[arg(long)]
        forward: Option<u64>,
        #[arg(long)]
        backward: Option<u64>,
    },

    Shuffle {
        #[arg(long)]
        on: bool,
        #[arg(long)]
        off: bool,
        #[arg(long)]
        toggle: bool,
    },

    Loop {
        #[arg(long)]
        none: bool,
        #[arg(long)]
        track: bool,
        #[arg(long)]
        playlist: bool,
    },
}

//
// ===== Requests =====
//

pub fn to_request(cli: &Cli) -> Result<Option<Request>> {
    Ok(Some(match &cli.cmd {
        Cmd::Ping => Request::Ping,

        Cmd::List { filter, .. } => Request::List {
            filter: filter.clone(),
        },

        Cmd::Status { player, .. } => Request::Status {
            target: parse_selector(player)?,
        },

        Cmd::Metadata { player, format, .. } => Request::Metadata {
            target: parse_selector(player)?,
            format: format.clone(),
            compat: None,
        },

        Cmd::Follow { player, format, .. } => Request::Follow {
            target: parse_selector(player)?,
            format: format.clone(),
            compat: None,
        },

        Cmd::Command { player, cmd } => Request::Command {
            target: parse_selector(player)?,
            cmd: control_to_ipc(cmd)?,
        },

        Cmd::Cache { .. } => return Ok(None),
    }))
}

//
// ===== Cache commands (client-side) =====
//

pub async fn handle_cache(cmd: CacheCmd) -> Result<()> {
    let cache = ImageCache::new()?;

    match cmd {
        CacheCmd::Info => {
            let (count, size) = cache.stats().await?;
            println!("Cache directory: {}", cache.root().display());
            println!("Images cached: {count}");
            println!("Cache size: {size} bytes");
        }

        CacheCmd::Clean => {
            cache.clear().await?;
            println!("Album art cache cleared.");
        }
    }

    Ok(())
}

//
// ===== Output =====
//

const DEFAULT_METADATA_FORMAT: &str = "{status} {artist} - {title}";

pub fn print_metadata(
    snap: &PlayerSnapshotOut,
    format: Option<&str>,
    mode: OutputMode,
) -> Result<()> {
    match mode {
        OutputMode::Json => println!("{}", serde_json::to_string_pretty(snap)?),
        OutputMode::Toml => println!("{}", toml::to_string(snap)?),
        OutputMode::Text => {
            let tpl = format.unwrap_or(DEFAULT_METADATA_FORMAT);
            let out = crate::output::format_template(tpl, snap);
            println!("{out}");
        }
    }
    Ok(())
}

//
// ===== Helpers =====
//

fn parse_selector(s: &str) -> Result<Target> {
    let (kind, filter) = match s.split_once(':') {
        Some((k, f)) => (k, Some(f.to_string())),
        None => (s, None),
    };

    Ok(match kind {
        "best" => Target::Best { filter },
        "all" => Target::All { filter },
        other => Target::Player {
            id: other.to_string(),
        },
    })
}

fn control_to_ipc(cmd: &ControlCmd) -> Result<Command> {
    Ok(match cmd {
        ControlCmd::Play => Command::Play,
        ControlCmd::Pause => Command::Pause,
        ControlCmd::PlayPause => Command::PlayPause,
        ControlCmd::Stop => Command::Stop,
        ControlCmd::Next => Command::Next,
        ControlCmd::Previous => Command::Previous,

        ControlCmd::Open { uri } => Command::Open { uri: uri.clone() },

        ControlCmd::Volume { set, up, down } => Command::Volume {
            level: *set,
            up: *up,
            down: *down,
        },

        ControlCmd::Position {
            set,
            forward,
            backward,
        } => Command::Position {
            set_to: *set,
            forward: *forward,
            backward: *backward,
        },

        ControlCmd::Shuffle { on, off, toggle } => {
            let state = match (*on, *off, *toggle) {
                (true, false, false) => Some(ShuffleState::On),
                (false, true, false) => Some(ShuffleState::Off),
                (false, false, true) => Some(ShuffleState::Toggle),
                (false, false, false) => None,
                _ => anyhow::bail!("choose at most one of --on/--off/--toggle"),
            };
            Command::Shuffle { state }
        }

        ControlCmd::Loop {
            none,
            track,
            playlist,
        } => {
            let state = match (*none, *track, *playlist) {
                (true, false, false) => Some(LoopState::None),
                (false, true, false) => Some(LoopState::Track),
                (false, false, true) => Some(LoopState::Playlist),
                (false, false, false) => None,
                _ => anyhow::bail!("choose at most one of --none/--track/--playlist"),
            };
            Command::Loop { state }
        }
    })
}
