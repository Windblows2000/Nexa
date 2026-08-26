### Nexa
---------------------------------------------

### Description

A modern media controller written in Rust. Nexa works with any MPRIS-compatible media player (Spotify, MPV, Firefox, Chromium, etc.). This project was born out of the desire for a "better" media controller. Especially in window managers where your only option is [Playerctl](https://github.com/altdesktop/playerctl).

## Features
- **Blazingly fast** — Daemon + CLI design for maximum speed.
- **Event-driven** — Subscribes to daemon signals instead of polling for metadata updates.
- **Real-time progress** — Efficient local ticker provides smooth, accurate playback progress.
- **Integrated album art caching** — A single command exposes a stable local path to album art / thumbnails.
- **Structured output** — Supports JSON (default), TOML, and template-based text output via `--format`.

## Examples
  ```bash
  nexa follow --toml #Prints out metadata in TOML
  nexa command volume --set 0.60 #Sets audio to 60%
  nexa command play-pause #Play/Pause the media
  nexa metadata # Prints out metadata in JSON
  nexa metadata --format '{artist} - {title}' #Playerctl-like output
  nexad #Starts the daemon.
  ```

## CLI


```bash
❯ nexa
A Powerful, Rust-Based CLI Linux Tool for your Media Needs.

Usage: nexa [OPTIONS] <COMMAND>

Commands:
  list      List detected MPRIS players
  status    Show current playback status
  metadata  Print current metadata once
  follow    Stream metadata updates
  command   Send a playback control command
  cache     Inspect or manage the album art cache
  ping      Ping the daemon
  help      Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose...  Increase logging verbosity (-v, -vv, -vvv)
  -h, --help        Print help
  -V, --version     Print version
```
## Custom Output Formatting (--format)

The --format option allows you to define custom text output using a
template string. Any text is printed verbatim, while placeholders wrapped
in {} are replaced with live metadata from the active player.

example:
```bash
nexa metadata --format "Title: {title} ({elapsed}/{length})"
```

### Available Fields
Track Information
| Field      | Description  |
| ---------- | ------------ |
| `{title}`  | Track title  |
| `{artist}` | Track artist |
| `{album}`  | Album name   |

Playback State
| Field      | Description                                      |
| ---------- | ------------------------------------------------ |
| `{status}` | Playback status (`Playing`, `Paused`, `Stopped`) |
| `{player}` | Active MPRIS player bus name                     |

Time & Playback
| Field       | Description                              |
| ----------- | ---------------------------------------- |
| `{elapsed}` | Elapsed playback time (MM:SS or H:MM:SS) |
| `{length}`  | Total track duration                     |
| `{rate}`    | Playback speed multiplier                |

Volume & Modes
| Field       | Description                                  |
| ----------- | -------------------------------------------- |
| `{volume}`  | Volume as a floating-point value (`0.0–1.0`) |
| `{shuffle}` | Shuffle state (`on` / `off`)                 |
| `{loop}`    | Loop mode (`None`, `Track`, `Playlist`)      |

Artwork
| Field        | Description               |
| ------------ | ------------------------- |
| `{art_url}`  | Remote artwork URL        |
| `{art_path}` | Local cached artwork path |



## Getting Started

It's easy! Just follow the instructions below:

### AUR

```bash
yay -S nexa-bin
```
or

```bash
paru -S nexa-bin
```
The package includes a systemd service for the daemon. Activate it with `systemctl --user enable --now nexad.service`

### Pre-built Binary

Grab the compiled binary from [here](https://github.com/Windblows2000/Nexa/releases).

### Build From Source

If you rather use Cargo or remain on the latest commits, 

clone the repo and install Nexa.
```bash
git clone https://github.com/Windblows2000/Nexa.git
cd Nexa
cargo install --path .
```

## Contributing

Contributions are welcome!

### Pull Requests

* Fork the repository

* Make your changes

* Submit a pull request

### Issues

Please include as much relevant information as possible (logs, output,
steps to reproduce, etc.).


## License

This project is licensed under the **GPLv3** license.
