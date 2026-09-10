# Design goals

Nexa began with a simple idea: controlling an MPRIS player should work well from a terminal, a status bar, or any other small desktop client without making each client manage D-Bus on its own.

That idea led to the daemon and client design. nexad handles MPRIS and keeps state. nexa provides a command-line interface to that state. Other clients can use the same protocol later.

This document describes the choices behind that design. It is not a promise that the implementation will never change.

## Keep one view of player state

MPRIS exposes players independently. A machine can have several of them, and their D-Bus properties can change at any time. A client that runs once can simply query the current values. A client that stays open must also discover new players, remove closed ones, subscribe to changes, and decide which player matters.

Nexa does this work in the daemon. The daemon maintains one shared view of all known players and makes that view available through IPC.

This avoids a situation where the CLI, a panel widget, and a notification process each reach different conclusions about the active player. They can all ask nexad for the same result.

## Treat the CLI as a client

The nexa executable is not the owner of media state. It is a client that happens to ship with the project.

For most commands, the CLI parses its arguments into an IPC request and prints the daemon's response. It does not contain a second MPRIS monitoring implementation.

Keeping that boundary clear makes the CLI easier to change and leaves room for other interfaces. A graphical client or status bar module should not need to copy the daemon's player-selection and caching logic.

Some operations are intentionally local. Generating shell completions does not need the daemon. Inspecting or clearing the album art cache can also operate directly on the cache directory. The daemon boundary is useful where shared state is involved, not as a rule that everything must cross IPC.

## Use MPRIS events when they are available

The daemon listens for property changes, seek events, and D-Bus name changes. It does not repeatedly scan every property to discover whether something happened.

This keeps updates quick and reduces unnecessary D-Bus traffic. A metadata change can be forwarded to a follow client as soon as the player reports it.

Playback position needs different treatment because players do not normally emit a signal for every passing second. Nexa estimates position from a local anchor and starts a ticker only when a client asks for timed output. The exception is kept narrow rather than turning the whole system into a polling loop.

## Make common commands short

The normal case should not require the user to know a D-Bus service name.

```
nexa metadata
nexa command play-pause
nexa follow
```

These commands operate on the daemon's best player. More specific controls remain available for machines running several players.

```
nexa command --player org.mpris.MediaPlayer2.spotify next
nexa command --all pause
```

The default is meant to be useful and convenient, not to remove control from users who need an exact target.

## Return complete snapshots

Follow clients receive complete output snapshots rather than a stream of small patches such as "title changed" or "volume changed."

A complete snapshot is larger, but it is easier to use. A client can render each message as it arrives without maintaining a second copy of daemon state or deciding how several events fit together.

The daemon already keeps state and combines partial MPRIS updates. Requiring every client to repeat that work would weaken the reason for having a daemon.

## Keep output useful in shell environments

Nexa is intended to work in window-manager setups, shell scripts, status bars, and other environments where output becomes input to another program.

JSON is the default structured format. TOML is available when it fits the surrounding tooling better. Templates provide compact text output without requiring an extra parser.

```
nexa metadata --format '{artist} - {title}'
nexa follow --format '{status}: {title} ({elapsed}/{length})'
```

Formatting belongs to the CLI. The daemon sends typed snapshot data and does not decide how a client presents it.

## Resolve artwork once

Album art often needs more work than reading a metadata string. It may be remote, embedded in a data URL, or represented as a file URI. A client may need a local path even when the player supplies an HTTP URL.

The daemon resolves and caches this artwork so that clients do not download the same image independently. The output includes both an artwork URL and a local path where possible.

Caching also gives short-lived clients a stable file to use after their IPC request has completed.

## Avoid permanent background work

A daemon is expected to run for a long time, so small amounts of unnecessary work accumulate.

Nexa keeps the progress ticker disabled unless a connected follow client needs it. It shares in-flight artwork downloads instead of starting several requests for the same URL. It stores current player state so that ordinary client queries do not need a fresh set of MPRIS calls.

The goal is not to remove every allocation or wake-up. The goal is to avoid work whose result nobody is using.

## Account for imperfect players

MPRIS defines a common interface, but players do not all expose the same optional properties or emit updates in the same way.

Nexa treats unavailable values as optional. It preserves an existing track ID when a partial metadata update omits it. It ignores small position differences but corrects larger jumps. It accepts several forms of album artwork.

These decisions belong in the daemon because they describe how Nexa understands MPRIS behavior. Clients should not need a separate collection of player-specific workarounds.

## Keep the protocol smaller than the implementation

The IPC protocol should contain what clients need, not every type used inside the daemon.

A client needs a target, a command, and a snapshot. It does not need the daemon's locks, timing anchors, activity priorities, monitor tasks, or D-Bus proxy types.

Keeping implementation details out of IPC makes those details easier to change. It also gives future clients a clearer interface than the daemon's internal module structure.

## Leave room for other clients

The bundled CLI is the first client and the reference for current protocol behavior. The daemon should also be usable by status bars, widgets, graphical applications, and other local tools.

This does not mean adding abstractions for clients that do not exist. It means avoiding assumptions that only a command-line process will connect, and keeping the daemon's useful state available through the IPC boundary.

The practical test is whether another local client can use Nexa without reimplementing MPRIS monitoring. If it can connect, request a snapshot, follow updates, and send commands, the daemon is providing the right boundary.

