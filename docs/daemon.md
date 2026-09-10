# The Nexa daemon

nexad is the part of Nexa that talks to media players. It runs in the user's session, connects to D-Bus, and listens for MPRIS activity.

Clients depend on the daemon for player discovery and state. If nexa cannot connect to the daemon, it reports that nexad is not running.

The daemon can be started directly:

```
nexad
```

Default is log level=info. Logging becomes more detailed with -v for debug and -vv for trace:

```
nexad -v
nexad -vv
```

Packaged installations may start the daemon through a user service instead.

## Starting the daemon

At startup, nexad creates the shared album art cache and connects to the session D-Bus. It then starts three long-running tasks: the player supervisor, the IPC server, and the playback ticker.

The supervisor and IPC server are essential. If either task exits, the daemon logs the event and shuts down. The ticker is a supporting task that remains idle until a client requests periodic elapsed-time updates.

The daemon currently uses Tokio's current-thread runtime. Individual player monitors and client connections still run as asynchronous tasks, but they share the same runtime thread.

## Discovering players

MPRIS players expose D-Bus names beginning with:

```
org.mpris.MediaPlayer2.
```

The supervisor asks D-Bus for the names that are already present when it starts. It creates a monitor for each matching name.

After the initial scan, the supervisor listens for name-owner changes from D-Bus. A new owner means that a player has appeared. The daemon starts a monitor for it immediately. When the owner disappears, the player is removed from shared state.

This allows nexad to remain running while players are opened and closed throughout the desktop session.

## Monitoring a player

A player monitor begins by creating an MPRIS proxy and reading a complete snapshot. That initial snapshot contains the player identity, playback status, track metadata, position, playback rate, volume, shuffle setting, and loop mode.

After the initial read, the monitor listens for property changes on the MPRIS player interface. It applies only the values included in each change signal. If a signal contains new metadata, for example, the daemon can update the metadata without replacing an unchanged volume or playback status.

Seek events are monitored separately. When a player emits the Seeked signal, Nexa updates its position anchor to the reported MPRIS position.

If the monitor ends, the player is removed from the daemon's state.

## Keeping state

The daemon stores players in a map keyed by their MPRIS bus names. Access to this map is guarded by an asynchronous read/write lock.

Readers can request the primary snapshot, a snapshot for a specific player, or the IDs of known players. Writers update a player when an MPRIS event arrives and recalculate which player should be primary.

Updates can also be broadcast. Before sending an update, the daemon creates a snapshot while it still holds the state lock. It releases the lock before publishing the snapshot so that a slow subscriber cannot block state access.

The broadcast channel has a bounded capacity. If a client falls behind, the server logs how many updates were missed and continues with newer snapshots.

## Tracking position locally

A live player does not store only a fixed position. It stores a position anchor and the time at which that anchor was set.

If the player is playing, Nexa adds the locally elapsed time to the anchor. The playback rate is included when one is available. If the player is paused or stopped, the anchor is returned unchanged.

The anchor is corrected when a new track starts, a seek is detected, or the reported position differs enough from the local estimate. Small differences are ignored so that harmless MPRIS timing variation does not cause constant corrections.

When playback changes from playing to another state, Nexa freezes the estimated position. When playback begins again, the timer resumes from the existing anchor.

## Selecting the primary player

The primary player is the target used by best.

Selection starts with playback status. A playing player wins over one that is paused or stopped. If both players are in the same class, Nexa compares their recent activity.

Status updates have the highest activity priority, followed by metadata updates and position updates. If the priority is equal, the player with the newer activity timestamp is preferred.

This is a selection rule, not a permanent assignment. The result can change when another player begins playing, receives a meaningful update, or leaves the bus.

## Handling commands

The IPC server passes ordinary requests to the command-handling layer. That layer resolves the requested target and creates the appropriate MPRIS proxy.

Playback commands such as play, pause, next, and previous map directly to MPRIS methods. Settings such as volume, shuffle, and loop mode may first require reading the current value so Nexa can apply a relative change or toggle.

Position commands use MPRIS microseconds internally, while the CLI accepts seconds. Setting an absolute position also requires the current track ID because the MPRIS SetPosition method associates a position with a particular track.

A command sent to all is applied to each matching player. Metadata and status requests require one player and therefore reject an all target.

## Serving follow clients

When the server receives a follow request, it subscribes to the state broadcast channel and sends the current matching snapshot.

The connection then remains inside the follow loop. Each new snapshot is checked against the target and the last snapshot sent to that client. Updates for unrelated players are skipped. For a best target, updates are skipped unless they belong to the current primary player.

The comparison includes the player, playback state, metadata, volume, shuffle mode, loop mode, rate, and position. While a track is playing, position changes are limited to one visible update per second. When the track is paused, unchanged position values do not produce additional output.

The follow loop ends when the client disconnects or the broadcast channel closes.

## Running the ticker on demand

Not every follow client needs regular elapsed-time updates. A template containing \{elapsed\} or \{position\} needs them, while a template containing only \{artist\} and \{title\} does not.

The client includes this requirement in its follow request. The server increments a shared demand count for the lifetime of the connection. A guard reduces the count automatically when the connection ends.

The ticker starts when the count becomes greater than zero and stops when it returns to zero. While active, it wakes once per second and asks the state to rebroadcast the primary snapshot. A snapshot is only sent if there are receivers and the primary player is playing.

## The Unix socket

The preferred socket location is:

```
$XDG_RUNTIME_DIR/nexa/daemon.sock
```

If XDG\_RUNTIME\_DIR is unavailable, Nexa uses its per-user cache directory. A final fallback under /tmp exists for environments where neither location can be determined.

Before binding, the daemon creates the parent directory and removes an old socket file at the same path. The new socket is assigned mode 0600, which restricts access to the current user.

The server accepts multiple connections. Each connection is handled in a separate asynchronous task and may contain ordinary requests or one follow stream.

## Album art cache

The daemon accepts artwork from HTTP, HTTPS, file, and embedded data sources. Remote downloads and decoded data URLs are stored in the user's Nexa cache.

A remote object is limited to 10 MiB. Nexa checks the advertised content length when available and also enforces the limit while reading the response. The download is first written to a temporary file and renamed after it completes.

Concurrent requests for the same URL share an in-flight lock. One task performs the download while the others wait and reuse the completed file.

The cache is allowed to grow to approximately 1 GB. When cleanup is needed, older files are removed until the total size is below the limit.

The CLI exposes basic cache maintenance without going through the daemon:

```
nexa cache info
nexa cache clean
```

