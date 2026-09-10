# Architecture

Nexa is an MPRIS controller built around a long-running daemon. The daemon, `nexad`, connects to media players through D-Bus, maintains a shared view of their state, and exposes that state through a local IPC protocol.

The included `nexa` command is one client of that protocol. Rather than discovering players and reading MPRIS properties itself, it turns command-line arguments into IPC requests, sends them to `nexad`, and prints the response.

This design gives commands, status bars, widgets, and other desktop integrations a consistent view of media playback while avoiding duplicate player discovery, monitoring, and album-art handling in every client.

This separation is the main architectural idea behind Nexa.

```
┌──────────────────────────────────┐
│          Media players           │
│ Spotify, MPV, Firefox, VLC, ...  │
└────────────────┬─────────────────┘
                 │ MPRIS over D-Bus
                 ▼
┌──────────────────────────────────┐
│              nexad               │
│                                  │
│  Player discovery and monitoring │
│  Shared player state             │
│  Primary-player selection        │
│  Command handling                │
│  Album art cache                 │
│  IPC server                      │
└────────────────┬─────────────────┘
                 │ Unix socket
                 ▼
┌──────────────────────────────────┐
│             Clients              │
│                                  │
│  nexa CLI                        │
│  Status bars and widgets         │
│  Other local applications        │
└──────────────────────────────────┘
```

## Why use a daemon?

A small MPRIS command can connect to D-Bus, find a player, read a property, print it, and exit. That works well for occasional commands. It becomes less useful when several programs need to watch the same players or when output needs to update continuously.

Nexa keeps one process connected to D-Bus instead. That process discovers players as they appear, subscribes to their updates, and stores their latest state in memory. A client can then ask the daemon for a snapshot without repeating the discovery and setup work.

The daemon also gives all clients the same view of the system. Player selection, playback progress, and album art resolution happen in one place rather than being implemented separately by every client.

## Player monitoring

The daemon discovers MPRIS services on the user's session bus. At startup it begins monitoring every player that is already running. It then watches D-Bus for players that appear or disappear later.

Each player has its own monitoring task. That task reads the initial MPRIS state and listens for property changes and seek events. Changes to metadata, playback status, volume, shuffle, loop mode, and position are applied to the daemon's shared state.

A player is removed from shared state when its MPRIS service leaves the bus.

## Shared state

The daemon stores one LivePlayer for each known player. A live player contains the latest metadata and playback settings, along with enough timing information to estimate the current position.

Clients receive snapshots rather than direct access to this state. A snapshot is a copy of the values that are useful outside the daemon, including the player ID, playback status, metadata, elapsed time, volume, shuffle mode, loop mode, and resolved artwork.

Keeping mutable state inside the daemon means clients do not need to combine several MPRIS properties or account for updates arriving at different times.

## Choosing a player

Most Nexa commands use the best player unless another target is given. The daemon chooses this player from its current state.

A playing player is preferred over a paused or stopped player. When two players have the same playback status, Nexa considers the kind of activity seen most recently and then the time of that activity. Playback-status changes have more weight than metadata or position updates.

This selection is recalculated whenever a player is added, changed, or removed. A client can therefore ask for the best player without keeping its own player history.

A client may also address a player by its complete MPRIS bus name:

```
nexa metadata --player org.mpris.MediaPlayer2.spotify
```

Commands may be sent to all available players:

```
nexa command -a pause
```

## Requests and subscriptions

Most client interactions use one request and one response. For example, nexa metadata connects to the socket, requests a snapshot, prints the response, and exits.

The follow command is different. Its connection remains open so that the daemon can send new snapshots as state changes.

```
nexa follow --format '{artist} - {title}'
```

The daemon sends the current snapshot first. It then forwards matching updates from its broadcast channel. Repeated snapshots are filtered so that a client is not notified when nothing useful has changed.

Follow mode is still driven by MPRIS events. It does not repeatedly query D-Bus for metadata.

## Playback position

MPRIS players usually report a position when queried, but they do not emit a new position every second. Asking D-Bus for the position continuously would create work that can be avoided.

Nexa records a position together with the instant when it was observed. While the player is playing, the current position is calculated from that anchor, the elapsed local time, and the playback rate. When the player pauses, seeks, or changes tracks, the anchor is updated.

A follow client may request regular position output. In that case, the daemon starts a timer aligned to one-second boundaries and rebroadcasts the current snapshot while the selected player is playing. The timer stops when no clients require it.

## Album art

MPRIS metadata may contain a remote URL, a local file URL, or an embedded data URL for album art. The daemon handles these forms before returning a snapshot to a client.

Remote images are downloaded to Nexa's cache. Embedded images are decoded into cache files. Local file URLs are converted to paths when possible. The resulting snapshot can include both the original or normalized URL and a stable local path.

This is why a status bar can use `{art_path\` without implementing an HTTP client or decoding embedded image data itself.

## Source layout

The main daemon components are under src/daemon. The supervisor monitors MPRIS players, the state module owns live player state, and the server accepts IPC connections.

The public message types and serialization code are under src/ipc. MPRIS proxies and metadata parsing are in src/mpris.rs. Command handling lives in src/control.rs.

The two executable entry points are src/bin/nexad.rs and src/bin/nexa.rs. Most of their behavior is implemented in the library so it can be shared and tested.

