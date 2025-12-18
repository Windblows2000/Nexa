### Nexa
---------------------------------------------

### Description

A modern media controller written in rust. This project was born out of the desire for a "better" media controller. Especially in window managers where your only option is [Playerctl.](https://github.com/altdesktop/playerctl)

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

Its super easy! just make sure you run the daemon in the background either via a systemd service or an exec command. An example in hyprland:
```
exec-once = nexad &
```
The daemon will run on boot and handle discovery and commands. If you're confused about any part of the CLI, always use ```nexa COMMAND --help``` to get more explanation.

### Contributing

For contributions, please read below.

* For pull requests:

1- fork the repo

2- create the changes

3- submit a pull request (PR)

* For issues:
Please provide as much information as possible in your issue (logs, output, etc...). 


### License

This project is licensed under the **GPLv3** license.
