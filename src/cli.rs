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

#[derive(Args, Debug, Clone, Copy)]
pub struct OutputArgs {
    /// Output the result in TOML format
    #[arg(long)]
    pub toml: bool,
}

impl OutputArgs {
    pub fn print(&self, snap: &PlayerSnapshotOut, format: Option<&str>) -> Result<()> {
        if self.toml {
            println!("{}", toml::to_string(snap)?);
        } else if let Some(tpl) = format {
            println!("{}", crate::output::format_template(tpl, snap));
        } else {
            println!("{}", serde_json::to_string_pretty(snap)?);
        }

        Ok(())
    }
}

#[derive(Args, Debug, Clone)]
pub struct PlayerArgs {
    /// The player to control
    #[arg(long, short, default_value = "best", conflicts_with = "all")]
    pub player: String,

    /// Query/control all available players
    #[arg(long, short)]
    pub all: bool,
}

/// A Powerful, Rust-Based CLI Linux Tool for your Media Needs.
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Cli {
    /// Increase logging verbosity (-v for debug, -vv for trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// List all currently available MPRIS players
    List {
        #[arg(long)]
        filter: Option<String>,
        #[command(flatten)]
        out: OutputArgs,
    },
    /// Generate shell completion scripts
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Get the current playback status of a player
    Status {
        #[command(flatten)]
        player_args: PlayerArgs,
        #[command(flatten)]
        out: OutputArgs,
    },
    /// Get full metadata for the current track
    Metadata {
        #[command(flatten)]
        player_args: PlayerArgs,
        #[arg(long)]
        format: Option<String>,
        #[command(flatten)]
        out: OutputArgs,
    },
    /// Continuously follow player updates and stream snapshots
    Follow {
        #[command(flatten)]
        player_args: PlayerArgs,
        #[arg(long)]
        format: Option<String>,
        #[command(flatten)]
        out: OutputArgs,
    },
    /// Send a control command (play, pause, etc.) to a player
    Command {
        #[command(flatten)]
        player_args: PlayerArgs,
        #[command(subcommand)]
        cmd: ControlCmd,
    },
    Cache {
        #[command(subcommand)]
        cmd: CacheCmd,
    },
    Ping,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CacheCmd {
    /// Show cache statistics (size and count)
    Info,
    /// Clear all cached album art
    Clean,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ControlCmd {
    /// Resume playback
    Play,
    /// Pause playback
    Pause,
    /// Toggle between play and pause
    PlayPause,
    /// Stop playback
    Stop,
    /// Skip to the next track
    Next,
    /// Skip to the previous track
    Previous,
    /// Open a specific URI in the player
    Open {
        /// The URI (e.g., a URL or file path) to open
        uri: String,
    },
    /// Adjust or set the player volume
    Volume {
        /// Set absolute volume (0.0 to 1.0)
        #[arg(long)]
        set: Option<f64>,
        /// Increase volume by an amount
        #[arg(long)]
        up: Option<f64>,
        /// Decrease volume by an amount
        #[arg(long)]
        down: Option<f64>,
    },
    /// Adjust or set the playback position
    Position {
        /// Set absolute position in seconds
        #[arg(long)]
        set: Option<u64>,
        /// Seek forward by seconds
        #[arg(long)]
        forward: Option<u64>,
        /// Seek backward by seconds
        #[arg(long)]
        backward: Option<u64>,
    },
    /// Manage shuffle state
    Shuffle {
        /// Enable shuffle
        #[arg(long)]
        on: bool,
        /// Disable shuffle
        #[arg(long)]
        off: bool,
        /// Toggle shuffle state
        #[arg(long)]
        toggle: bool,
    },
    /// Manage loop/repeat state
    Loop {
        /// Disable looping
        #[arg(long)]
        none: bool,
        /// Loop the current track
        #[arg(long)]
        track: bool,
        /// Loop the current playlist
        #[arg(long)]
        playlist: bool,
    },
}

pub fn to_request(cli: &Cli) -> Result<Option<Request>> {
    let request = match &cli.cmd {
        Cmd::Cache { .. } | Cmd::Completions { .. } => return Ok(None),
        Cmd::Ping => Request::Ping,
        Cmd::List { filter, .. } => Request::List { filter: filter.clone() },
        Cmd::Status { player_args, .. } => Request::Status { target: target_from_args(&player_args.player, player_args.all)? },
        Cmd::Metadata { player_args, .. } => Request::Metadata { target: target_from_args(&player_args.player, player_args.all)? },
        Cmd::Follow { player_args, format, out, .. } => {
            let with_time = out.toml || format.as_deref().is_none_or(|tpl| tpl.contains("{elapsed}") || tpl.contains("{position}"));
            Request::Follow { target: target_from_args(&player_args.player, player_args.all)?, with_time }
        }
        Cmd::Command { player_args, cmd } => {
            Request::Command { target: target_from_args(&player_args.player, player_args.all)?, cmd: control_to_ipc(cmd)? }
        }
    };

    Ok(Some(request))
}

pub async fn handle_cache(cmd: CacheCmd) -> Result<()> {
    let cache = ImageCache::new().await?;

    match cmd {
        CacheCmd::Info => {
            let (count, size) = cache.stats().await?;
            println!("Cache directory: {}", cache.root().display());
            println!("Images cached: {count}");
            println!("Cache size: {}", format_bytes(size));
        }
        CacheCmd::Clean => {
            let cache_path = cache.root().to_owned();
            cache.clear().await?;
            println!("Album art cache cleared at {}.", cache_path.display());
        }
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

fn target_from_args(player: &str, all: bool) -> Result<Target> {
    if all { Ok(Target::All { filter: None }) } else { parse_selector(player) }
}

fn parse_selector(s: &str) -> Result<Target> {
    let (kind, filter) = match s.split_once(':') {
        Some((k, f)) => (k, Some(f.to_string())),
        None => (s, None),
    };

    Ok(match kind {
        "best" => Target::Best { filter },
        "all" => Target::All { filter },
        other => Target::Player { id: other.to_string() },
    })
}

fn exactly_one_or_none(options: [bool; 3], err: &str) -> Result<()> {
    let selected = options.into_iter().filter(|selected| *selected).count();
    if selected > 1 {
        anyhow::bail!("{}", err);
    }
    Ok(())
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
        ControlCmd::Volume { set, up, down } => {
            exactly_one_or_none([set.is_some(), up.is_some(), down.is_some()], "choose at most one of --set/--up/--down")?;

            Command::Volume { level: *set, up: *up, down: *down }
        }
        ControlCmd::Position { set, forward, backward } => {
            exactly_one_or_none(
                [set.is_some(), forward.is_some(), backward.is_some()],
                "choose at most one of --set/--forward/--backward",
            )?;

            Command::Position { set_to: *set, forward: *forward, backward: *backward }
        }
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
        ControlCmd::Loop { none, track, playlist } => {
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
