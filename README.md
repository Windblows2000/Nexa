### Nexa
---------------------------------------------

### Description

A modern media controller written in Rust. This project was born out of the desire for a "better" media controller. Especially in window managers where your only option is [Playerctl](https://github.com/altdesktop/playerctl).

### Features
- **Blazingly fast** — Daemon + CLI design delivers sub-2 ms execution times.
- **Event-driven** — Subscribes to daemon signals instead of polling for metadata updates.
- **Real-time progress** — Efficient local ticker provides smooth, accurate playback progress.
- **Integrated album art caching** — A single command exposes a stable local path to album art / thumbnails.
- **Structured output** — Supports JSON (default), TOML, and template-based text output via `--format`.

### Examples
  ```bash
  nexa follow --toml #Prints out metadata in TOML
  nexa command volume --set 0.60 #sets audio to 60%
  nexa command play-pause #Play/Pause the media
  nexa metadata #Prints out metadata in json
  nexad #Starts the daemon.
  ```

### CLI

```
❯ nexa
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

Its super easy! Just make sure you run the daemon in the background either via a systemd service or an exec command. An example in hyprland:
```
exec-once = nexad &
```
The daemon will run on boot to handle discovery and commands. If you're confused about any part of the command line, always use ```nexa COMMAND --help``` to get more explanation.

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
