# IPC protocol

Nexa uses a local protocol between nexad and its clients. The protocol carries player queries, playback commands, snapshots, and follow streams.

The Rust types that define the protocol live under src/ipc. Although the included CLI is currently the main client, these types form the boundary between the daemon and any client that wants to use it.

## Connection and framing

The client connects to the daemon's Unix socket. Messages are carried in length-delimited frames, which allows the receiver to distinguish one serialized value from the next without inspecting the payload.

The server accepts frames up to 1 MiB. Requests and responses are encoded with Postcard.

A normal command creates a connection, sends one request, receives one response, and closes the connection:

```
client                         daemon
   │                              │
   │  Metadata { target: Best }   │
   ├─────────────────────────────>│
   │                              │
   │  Metadata(snapshot)          │
   │<─────────────────────────────┤
   │                              │
```

A follow request uses the same connection and framing but keeps the connection open for further responses.

## Message envelopes

Every encoded request and response is wrapped in an envelope. The envelope contains a protocol version and the actual payload.

The current version is defined in src/ipc/version.rs

The decoder checks the envelope before returning the payload. If the received version differs from the local version, decoding fails with a protocol mismatch.

The version is intended to change when the wire format changes in a way that existing clients cannot understand. Adding protocol features still requires care because Postcard serializes the Rust data model directly. Compatibility should be considered before changing enum variants, field order, or field types.

## Targets

Requests that operate on a player carry a Target.

Target::Player contains an exact MPRIS bus name. The daemon uses that name directly.

```
org.mpris.MediaPlayer2.spotify
```

Target::Best asks the daemon to use its current primary player. It may also contain a filter. A filter performs a case-insensitive substring match against the player bus name.

Target::All applies an operation to every matching player. It is meaningful for commands, but not for queries that must return one status or one metadata snapshot.

The CLI turns selectors into these target variants. The default selector is best.

```
nexa metadata
nexa metadata --player best:spotify
nexa metadata --player org.mpris.MediaPlayer2.spotify
nexa command --all pause
```

## Requests

Ping checks whether the daemon is reachable. It returns pong and carries no player information.

List returns the MPRIS bus names currently available through D-Bus. An optional filter limits the result to names containing the supplied text:

```
nexa list --filter spotify
```

Status resolves one target and returns its playback status as a string.

Metadata resolves one target and returns a complete output snapshot. Despite its name, the response includes playback settings and resolved artwork in addition to track metadata.

Command contains a target and a control command. Commands include transport controls, opening a URI, changing volume, seeking, changing shuffle, and changing loop mode.

Follow begins a stream of metadata snapshots. Its with\_time field tells the daemon whether the client needs periodic playback-position updates.

## Responses

A successful response depends on the request. It may contain Pong, a list of players, a status string, a metadata snapshot, a position, or an optional command result.

Errors are returned as Response::Error. The server converts internal request failures into readable strings before sending them to the client. The CLI treats an error response as a failed command.

A metadata snapshot contains the values intended for client output:

```
player_id
identity
status
title
artist
album
art_url
art_path
elapsed
length
rate
volume
shuffle
loop_status
```

Elapsed time and track length are sent as seconds. MPRIS positions are measured in microseconds inside the daemon, but clients do not need to use that representation for snapshots.

The artwork path may point to a local source file or to an object in Nexa's cache.

## Follow streams

A follow request changes the behavior of the connection. Instead of producing one response and returning to the request loop, the server moves the connection into follow mode.

The first response is the current snapshot for the selected target, when one is available. Later snapshots are sent when the daemon's shared state publishes a matching update.

```
client                         daemon
   │                              │
   │  Follow { target: Best }     │
   ├─────────────────────────────>│
   │                              │
   │  Metadata(snapshot)          │
   │<─────────────────────────────┤
   │                              │
   │  Metadata(snapshot)          │
   │<─────────────────────────────┤
   │                              │
   │  Metadata(snapshot)          │
   │<─────────────────────────────┤
   │                              │
```

The daemon does not send a separate event type for every MPRIS property change. It sends complete output snapshots. This keeps clients simple because each response can be rendered without combining it with earlier responses.

The stream lasts until the client disconnects, the socket fails, or the daemon's broadcast channel closes.

## Timed output

A follow request can set with\_time to true. The CLI does this when its output includes elapsed position or when the output format implies that full snapshots will be printed.

Timed follow connections contribute to the daemon's ticker demand count. While at least one such connection exists, the daemon rebroadcasts the playing primary snapshot once per second.

A client that only needs track or status changes can set with\_time to false. It will still receive event-driven updates, but it will not keep the ticker active.

## Socket location and access

The client and daemon use the same socket\_path function. The preferred path is:

```
$XDG_RUNTIME_DIR/nexa/daemon.sock
```

The daemon creates the socket with mode 0600. IPC is therefore local to the user account and does not include network transport, remote authentication, or multi-user access.

The protocol assumes that the client and daemon run in the same user session.

## Changing the protocol

Changes under src/ipc affect more than the daemon's internal implementation. They can change what existing clients send or expect to receive.

A compatible change should preserve the meaning and encoded representation of existing messages. A breaking change should increment PROTOCOL\_VERSION and update both the daemon and bundled CLI together.

Code that is private to the daemon should remain outside the IPC types. The protocol should describe what a client needs to request or observe, not how the daemon stores or calculates it.

