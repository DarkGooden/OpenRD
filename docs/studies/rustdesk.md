# Case Study — RustDesk

> Source material: github.com/rustdesk/rustdesk, github.com/rustdesk/rustdesk-server.

## Overview

RustDesk is a self-hostable, open-source remote-desktop tool written in
Rust. It launched in 2021 as an explicit alternative to TeamViewer and
AnyDesk, with a focus on:

- **Easy install and "just works"** UX (PIN-style 9-digit IDs).
- **Self-hosted relay** option for organizations that don't want to
  use the public RustDesk relay infrastructure.
- **Cross-platform clients** — Windows, macOS, Linux, Android, iOS,
  Web.

RustDesk is the closest existing project to OpenRD's mission profile.
Studying it is essential because, on paper, it is doing roughly what
OpenRD wants to do.

## Wire format / transport

RustDesk's wire protocol is **defined by its Protobuf schema** in
`libs/hbb_common/protos/` — the protocol is effectively "whatever the
RustDesk client and server agree on this release." There is no
human-readable specification document.

Transport stack:

- **Custom framing** over TCP and over UDP, with the same Protobuf
  payloads.
- **A relay/rendezvous server** in the public deployment (`rs-ny.rustdesk.com`
  by default) handles ID-to-peer mapping, NAT traversal, and optionally
  relays the full connection when direct peer-to-peer fails.
- **NAT traversal** via STUN-style hole-punching.
- **End-to-end encryption** is supported but historically had to be
  enabled and the defaults were... evolving. Recent versions encrypt
  by default.

## Channel model

Single transport, multiplexed via Protobuf message types. Categories:

- Login / session negotiation
- Video frame (encoded VP9 or H.264 / H.265 / AV1 depending on
  capability)
- Audio (Opus)
- Mouse / keyboard events
- Clipboard
- File transfer (with explicit progress messages)
- Cursor position
- Misc control (resolution change, ctrl+alt+del injection, etc.)

There is *some* prioritization between message types but no QUIC-style
independent-stream multiplexing — large file transfers can interfere
with input responsiveness if not carefully managed.

## Security model

- **Public-key identification.** Each peer has a long-term keypair.
  The 9-digit ID is derived from the public key.
- **Connection authentication** via the peer's public key + a one-time
  password (the displayed PIN for casual support) or a permanent
  password set on the host.
- **End-to-end encryption** between client and host even when traffic
  is relayed.
- **The relay server is in principle untrusted** with respect to
  session content but does see metadata (who connects to whom).

The crypto is reasonable in modern versions but, again, custom rather
than TLS-based.

## Compression / codec

- **VP9** is the default video codec (broad browser support, royalty-free,
  good quality/bitrate).
- **H.264 / H.265 / AV1** available depending on platform and hardware
  encode availability.
- **Opus** for audio.
- **JPEG / raw fallback** for rare cases.

The codec choice is negotiated. Hardware encode is used when available,
software otherwise.

## What's good

1. **Self-hostable.** The whole stack — host, client, relay — can run
   on your own infrastructure with no cloud dependency. This is huge.
2. **Cross-platform with a working web client.** Few open projects
   actually deliver this matrix.
3. **PIN-based identity** is excellent for ad-hoc support scenarios.
4. **Built-in relay** with hole-punching first means it actually works
   across NATs without operator effort.
5. **File transfer is a first-class feature.**
6. **Rust.** Memory-safe network-facing daemon; the kind of thing a
   public protocol's reference implementation should be.
7. **Modern codec set** (VP9 default, AV1 optional).

## What's bad

1. **No public spec.** "The protocol" is whatever the latest Protobuf
   schema and server version do. This precludes independent
   implementations from interoperating with confidence. *This is the
   single biggest reason OpenRD should exist.*
2. **Tightly coupled to its relay infrastructure.** The protocol is
   implicitly designed around the assumption that there is always a
   rendezvous service available. Direct LAN-only operation works but is
   less smooth.
3. **Limited backwards-compatibility story.** Schema changes between
   releases require matching client and server versions in practice.
4. **No QUIC.** TCP/UDP custom framing instead of building on the
   standard.
5. **Single-stream multiplexing** means file transfers can affect
   interactive responsiveness.
6. **Crypto is custom-rolled** rather than TLS. Has been audited
   informally but not by a major firm to public-spec standards.
7. **Performance is good but not Sunshine-level.** The codec pipeline
   is not tuned to the same latency targets as a gaming protocol.

## What OpenRD should copy

1. **PIN-based pairing UX** as one auth mode.
2. **Self-hosting as a first-class deployment model.** Federation-free,
   no required cloud component.
3. **File transfer as a core, peer-of-display channel.**
4. **Rust as the reference-implementation language.**
5. **Cross-platform client set as a v1 commitment.**
6. **Public-key-based peer identity** that survives IP changes.

## What OpenRD should do differently

1. **Write a real public specification first**, code second. RustDesk's
   single biggest gap.
2. **Use QUIC**, not custom UDP framing. Independent streams per channel
   so file transfers don't fight input.
3. **TLS 1.3 (via QUIC) instead of custom crypto.**
4. **Make the rendezvous/relay sub-protocol optional and clearly
   separated** from the core peer-to-peer protocol. v0 has no relay at
   all (NG8); v1 may add one. RustDesk's design implicitly assumes one,
   which is the source of several of its complications.

## References

- RustDesk: github.com/rustdesk/rustdesk
- RustDesk Server: github.com/rustdesk/rustdesk-server
- Protobuf schemas: github.com/rustdesk/rustdesk/tree/master/libs/hbb_common/protos
