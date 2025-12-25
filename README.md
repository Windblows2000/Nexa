### Nexa
---------------------------------------------

### Description

A modern media controller written in Rust. Nexa works with any MPRIS-compatible media player (Spotify, MPV, Firefox, Chromium, etc.). This project was born out of the desire for a "better" media controller. Especially in window managers where your only option is [Playerctl](https://github.com/altdesktop/playerctl).

### Features
- **Blazingly fast** — Daemon + CLI design for maximum speed.
- **Event-driven** — Subscribes to daemon signals instead of polling for metadata updates.
- **Real-time progress** — Efficient local ticker provides smooth, accurate playback progress.
- **Integrated album art caching** — A single command exposes a stable local path to album art / thumbnails.
- **Structured output** — Supports JSON (default), TOML, and template-based text output via `--format`.

### Examples
  ```bash
  nexa follow --toml #Prints out metadata in TOML
  nexa command volume --set 0.60 #Sets audio to 60%
  nexa command play-pause #Play/Pause the media
  nexa metadata # Prints out metadata in JSON
  nexa metadata --format '{artist} - {title}' #Playerctl-like output
  nexad #Starts the daemon.
  ```

### CLI

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

### Getting Started

It's easy! Just follow the instructions below:

1- clone the repo and install Nexa.
```bash
git clone https://github.com/Windblows2000/Nexa.git
cd Nexa
cargo install --path .
```

2- The daemon is preferably started via the supplied systemd user service.
```bash
mkdir -p ~/.config/systemd/user/
cp resources/systemd/nexad.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now nexad.service
```
The daemon runs per-user and automatically tracks media players as they appear or disappear.

### Contributing

For contributions, please read below.

* For pull requests:

1- Fork the repo

2- Create the changes

3- Submit a pull request (PR)

* For issues:

Please provide as much information as possible in your issue (logs, output, etc...). 


### License

This project is licensed under the **GPLv3** license.
