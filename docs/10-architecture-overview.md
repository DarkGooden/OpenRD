# 10 — Architecture Overview

> Status: Draft v0.1
> Last updated: 2026-05-13

This document is the synthesis of [`Tier 1`](00-goals-and-non-goals.md)
and the [`case studies`](studies/). It defines the boxes, the
responsibilities, the data flow, and the lifecycles of an OpenRD
deployment.

## The high-level picture

```
+----------------------+                          +----------------------+
|       CLIENT         |                          |        SERVER        |
|  (Windows / macOS /  |        QUIC + TLS 1.3    |    (Linux only,      |
|   Web / Android /    |  <--------------------- >|        v0)           |
|      iOS)            |    one connection,       |                      |
|                      |    many streams          |                      |
|  +----------------+  |                          |  +----------------+  |
|  | Client core    |  |                          |  | Server core    |  |
|  | (state machine,|  |                          |  | (state machine,|  |
|  |  channels)     |  |                          |  |  channels)     |  |
|  +----------------+  |                          |  +----------------+  |
|         |            |                          |         |            |
|  +------+-------+    |                          |  +------+-------+    |
|  | Renderer /   |    |                          |  | Capture /    |    |
|  | Input /      |    |                          |  | Encoder /    |    |
|  | OS hooks /   |    |                          |  | Input inject |    |
|  | Web glue     |    |                          |  | / Sound /    |    |
|  +--------------+    |                          |  | FS hooks     |    |
+----------------------+                          +----------------------+
                                                            |
                                                            v
                                                   +----------------------+
                                                   |  Desktop session    |
                                                   |  (X11 / Wayland /   |
                                                   |   Xorg-on-virtual)  |
                                                   +----------------------+
```

There are **two boxes** in v0: client and server. There is **no relay**,
**no rendezvous service**, **no central directory**. The client knows
the server's address (FQDN or IP, port) and a credential (token or
client cert). It connects. That's it.

This is a deliberate simplification (see
[`NG8`](00-goals-and-non-goals.md)). Relays and NAT traversal are a
v1 concern.

## Components

### Client core

Platform-independent code, intended to be the same on every supported
platform. Responsibilities:

- Transport (QUIC) lifecycle.
- Capability negotiation.
- Channel management (open, close, route).
- Decode of inbound display frames.
- Encode of input events into the wire format.
- Session state and resumption.

The client core is built as a **library**, exposing a small C ABI for
embedding into native apps and a TypeScript binding (compiled from
the same Rust code via WASM) for the web client.

### Platform glue

Per-platform code that adapts the client core to the host environment:

- **Windows / macOS**: native windowing, OS clipboard hooks, OS
  filesystem access, hardware decode where available.
- **Web**: HTML `<video>` element + WebCodecs / Media Source
  Extensions, browser clipboard API, File System Access API where
  available, fall back to download/upload.
- **Mobile**: touch input handling, on-screen keyboard, app
  lifecycle hooks (background/foreground).

### Server core

Platform-independent code that does most of the real work.
Responsibilities:

- QUIC server endpoint.
- Authentication (delegating to a pluggable auth backend).
- Session state, including the resumption window.
- Channel management.
- Per-session capture / encode / stream of the display.
- Per-session input injection.
- Audio capture and encode (Opus).
- File channel handling.
- Clipboard channel handling.

### Server platform hooks

Linux-specific code (in v0) for:

- **Display capture.** Multiple backends:
  - **Headless**: Xvfb / Wayland virtual compositor (preferred for
    cloud / VM use cases — capture is direct from the compositor).
  - **PipeWire**: capture an existing physical session via PipeWire
    portal (preferred for desktop user assistance).
  - **X11**: XCB / XDamage on existing physical X session
    (legacy fallback).
- **Input injection.** `uinput` for synthesizing keyboard / mouse /
  touch into the kernel; `xdotool`-equivalent paths for X11-only
  setups.
- **Audio capture.** PipeWire / PulseAudio loopback.
- **Filesystem.** Server runs as a user; transfers happen against
  that user's filesystem permissions.

### Pluggable auth backend

The server core does not authenticate users itself. It calls into a
pluggable auth backend with a credential (token, client cert, PIN)
and gets back an authenticated identity + permission level (view-only
or interactive).

Reference backends shipped with the server:

- **File-based** — tokens + passwords in a file, for development.
- **OIDC** — verify a bearer token against an OIDC provider.
- **mTLS** — accept any client cert signed by a configured CA.
- **PIN** — single-use 9-digit codes for support scenarios.

## Data flow: a steady-state session

```
USER PRESSES "A"
    |
    v
[Client OS] -- keypress --> [Client core]
                                |
                                | Input channel (QUIC stream 3)
                                v
                       [Server core] -- uinput --> [Linux kernel]
                                                        |
                                                        v
                                                   [Desktop apps]
                                                        |
                                                  framebuffer changes
                                                        |
                                                        v
                       [Server core: capture + encode] --
                                |
                                | Display channel (QUIC stream 1)
                                v
                       [Client core: decode] -- frame --> [Client renderer]
                                                                |
                                                                v
                                                          USER SEES "A"
```

Target end-to-end latency: < 30 ms on LAN, < 100 ms on a 50 Mbps WAN
with 20 ms RTT (see NF-1).

## Session lifecycle

```
   DISCONNECTED
        |
        | client initiates
        v
   QUIC HANDSHAKE
        |
        v
   AUTH HANDSHAKE  ----failure---> DISCONNECTED
        |
        v
   CAPABILITY NEGOTIATION
        |
        v
   READY  <----+
    |  ^      |
    |  |      | network blip
    |  +------+
    | normal close
    v
   CLOSING -----> CLOSED
```

The `READY` state is the steady state. Network blips trigger
transport-layer retransmit (QUIC handles this). A longer outage
(> ~5 s of dead air) trips a session-resumption attempt — the client
opens a new QUIC connection and presents the session ID and a resumption
token to skip auth and re-attach to the live session on the server.

After the resumption window (NF-3.3, default 30 s) expires, the
server tears down the session and the client must reconnect from
scratch.

## Channel lifecycle

Channels are opened by either side after `READY`, via a `OpenChannel`
control message:

```
client                            server
  |                                  |
  |---- OpenChannel(kind, ...) ----->|
  |                                  |
  |<--- OpenChannelAck(stream_id) ---|
  |                                  |
  |  ... data on stream_id ...       |
  |                                  |
  |---- CloseChannel(stream_id) ---->|
  |                                  |
  |<--- CloseChannelAck -------------|
```

Each channel lives on its own QUIC stream pair (one bidirectional or
one or two unidirectional, depending on the channel's directionality).
Closing a channel doesn't affect any other channel.

## Deployment topologies

OpenRD v0 supports two deployment shapes:

### Single-tenant on a workstation

The user runs an OpenRD server on their workstation; another user
connects to it from a client. Auth is typically PIN-based or a
shared token. This is the UC-2 (support tech) and UC-1 (sysadmin
SSHing into a friend's box) shape.

### Multi-tenant on a server

An organization runs OpenRD servers on a fleet (think: Linux VMs
in a data center). Each user has credentials managed by the
organization's auth backend (OIDC, LDAP via a connector). A user
connects to a specific server; the server validates the credential
against the backend.

There is no central "OpenRD admin server." Authorization is per-host.
Multi-tenant fleets are managed by the operator's existing tools.

## Threading and concurrency model (reference implementation)

This is implementation guidance, not protocol normative. Implementations
are free to differ.

- One tokio runtime, multi-threaded.
- One task per session.
- Per session: one task each for QUIC I/O, capture loop, encode loop,
  audio loop, file transfer pool. Tasks communicate via bounded MPSC
  channels.
- Backpressure: capture loop drops frames if encode is behind; encode
  loop drops frames if the wire is behind. Drop-policy is "newest
  wins" for video (skip a frame, don't queue it).

## What this overview does NOT specify

- The byte-level wire format → [`20-wire-format-v0.md`](20-wire-format-v0.md)
- The list of channels in detail → [`11-channel-model.md`](11-channel-model.md)
- The transport choice rationale → [`12-transport-choice.md`](12-transport-choice.md)
- The state machines in detail → [`21-state-machines.md`](21-state-machines.md)
- The capability negotiation message format → [`22-capability-negotiation.md`](22-capability-negotiation.md)
