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

use crate::cache::ImageCache;
use crate::ipc::{Command, LoopState, PlayerSnapshotOut, Request, ShuffleState, Target};
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

//
// ===== Output =====
//

#[derive(Args, Debug, Clone, Copy)]
pub struct OutputArgs {
    /// Force TOML output
    #[arg(long)]
    pub toml: bool,
}

impl OutputArgs {
    pub fn print(&self, snap: &PlayerSnapshotOut, format: Option<&str>) -> Result<()> {
        if self.toml {
            println!("{}", toml::to_string(snap)?);
        } else if let Some(tpl) = format {
            let out = crate::output::format_template(tpl, snap);
            println!("{out}");
        } else {
            println!("{}", serde_json::to_string_pretty(snap)?);
        }
        Ok(())
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
    /// The shell to generate completions for.
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Show current playback status.
    Status {
        #[arg(long, default_value = "best", conflicts_with = "all")]
        player: String,

        /// Target all available players
        #[arg(long, short)]
        all: bool,

        #[command(flatten)]
        out: OutputArgs,
    },

    /// Print current metadata once.
    Metadata {
        #[arg(long, default_value = "best", conflicts_with = "all")]
        player: String,

        /// Target all available players
        #[arg(long, short)]
        all: bool,

        /// Template for text output
        #[arg(long)]
        format: Option<String>,

        #[command(flatten)]
        out: OutputArgs,
    },

    /// Stream metadata updates.
    Follow {
        #[arg(long, default_value = "best", conflicts_with = "all")]
        player: String,

        /// Target all available players
        #[arg(long, short)]
        all: bool,

        /// Template for text output
        #[arg(long)]
        format: Option<String>,

        #[command(flatten)]
        out: OutputArgs,
    },

    /// Send a playback control command.
    Command {
        #[arg(long, default_value = "best", conflicts_with = "all")]
        player: String,

        /// Target all available players
        #[arg(long, short)]
        all: bool,

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
        Cmd::Cache { .. } | Cmd::Completions { .. } => return Ok(None),

        Cmd::Ping => Request::Ping,

        Cmd::List { filter, .. } => Request::List {
            filter: filter.clone(),
        },

        Cmd::Status { player, all, .. } => Request::Status {
            target: if *all {
                Target::All { filter: None }
            } else {
                parse_selector(player)?
            },
        },

        Cmd::Metadata { player, all, .. } => Request::Metadata {
            target: if *all {
                Target::All { filter: None }
            } else {
                parse_selector(player)?
            },
        },

        Cmd::Follow {
            player,
            all,
            format,
            out,
            ..
        } => {
            let with_time = if out.toml {
                true
            } else if let Some(f) = format {
                f.contains("{elapsed}") || f.contains("{position}")
            } else {
                true
            };

            Request::Follow {
                target: if *all {
                    Target::All { filter: None }
                } else {
                    parse_selector(player)?
                },
                with_time,
            }
        }

        Cmd::Command { player, all, cmd } => Request::Command {
            target: if *all {
                Target::All { filter: None }
            } else {
                parse_selector(player)?
            },
            cmd: control_to_ipc(cmd)?,
        },
    }))
}

//
// ===== Cache commands (client-side) =====
//

pub async fn handle_cache(cmd: CacheCmd) -> Result<()> {
    let cache = ImageCache::new().await?;

    match cmd {
        CacheCmd::Info => {
            let (count, size) = cache.stats().await?;
            println!("Cache directory: {}", cache.root().display());
            println!("Images cached: {count}");
            println!("Cache size: {size} bytes");
        }

        CacheCmd::Clean => {
            let cache_path = cache.root().to_owned();
            cache.clear().await?;
            println!("Album art cache cleared at {}.", cache_path.display());
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
