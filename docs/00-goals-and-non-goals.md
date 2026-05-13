# 00 — Goals and Non-Goals

> Status: Draft v0.1
> Last updated: 2026-05-13

This document anchors every other design decision in OpenRD. When in doubt
about whether a feature, codec, or channel belongs in the protocol, return
here. Anything not listed as a goal is, by default, not a goal.

## Goals

### G1. Open, public, implementable specification
Anyone — individual or company — must be able to read the OpenRD spec and
write a conformant client or server from it alone, without reverse-engineering
a reference implementation. The spec is the source of truth; the reference
implementations are illustrative.

### G2. Productivity-first feature set
OpenRD targets the day-to-day workflows of IT administrators, support
technicians, remote workers, and developers. Concretely, in priority order:

1. Low-latency keyboard and mouse input
2. Bidirectional clipboard (text, formatted text, images)
3. File transfer (drag-and-drop and CLI), both directions, including
   large files (>1 GB) and folder trees
4. Audio (mono/stereo, low-bitrate by default)
5. Display (video stream of the remote desktop)

Note that display sits at the *bottom* of this list. Productivity users tolerate
a slightly softer image; they do not tolerate a broken paste or a failed file
transfer.

### G3. Sub-30ms input-to-feedback latency on a LAN
On a 1 Gbps LAN with < 1 ms RTT, the time between a keypress at the client
and the corresponding pixel arriving at the client's display must be under
30 ms, end-to-end, with CPU-only H.264 encode at 1080p60. This bounds every
other design decision (transport, codec, buffering, scheduling).

### G4. End-to-end confidentiality, integrity, and authentication
All channels are encrypted from client to server. No unencrypted fallback
exists in the protocol. Authentication is mutual and resistant to
man-in-the-middle attacks even when the underlying network is hostile.

### G5. Implementable in many languages and on many platforms
The wire format is byte-oriented, length-prefixed, and free of legacy
encodings (no ASN.1, no per-bit packing, no ITU-T T.x). A competent
developer should be able to implement a v0 client in any language that has
a QUIC library, a JSON or CBOR parser, and an H.264 decoder. The server
must be implementable on Linux; clients must be implementable on Windows,
macOS, Web (modern browsers), Android, and iOS.

### G6. Graceful degradation
A client with no H.264 hardware decode falls back to software decode. A
client behind a network that blocks UDP falls back to TCP+TLS. A client
that cannot negotiate audio still gets video, input, clipboard, and files.
A feature failing to negotiate must never break the session.

### G7. Easy to embed
OpenRD is intended to be embedded inside other software (the author's, and
others'). The reference client libraries expose a small, stable API; the
protocol does not require a particular UI framework, windowing system, or
authentication backend.

## Non-Goals

### NG1. Wire compatibility with Microsoft RDP
OpenRD is not a drop-in replacement for the MS RDP wire protocol. Existing
RDP clients (mstsc, FreeRDP, Remmina) will not connect to OpenRD servers.
A separate translating gateway could be written, but it is outside the
scope of the protocol itself.

### NG2. Gaming and high-frame-rate workloads
OpenRD is not optimized for 4K60+ gaming, GPU passthrough, HDR, VR, or
sub-10ms motion-to-photon. Users with those needs should use
Sunshine/Moonlight or Parsec.

### NG3. GPU encode as a hard requirement
The protocol is designed to run with CPU-only encode at 1080p60 with H.264.
GPU encode (NVENC, AMF, QuickSync, VAAPI) is a permitted optimization but
must never be required. Servers without a GPU must be first-class citizens.

### NG4. AV1, H.265, or any non-baseline codec in v0
The v0 spec mandates H.264 Baseline / Constrained Baseline and an optional
H.264 Main profile. AV1, H.265, VP9, and JPEG-XL may be added in later
versions via capability negotiation, but they are not part of v0. This is
deliberate: web client compatibility, broad hardware decode, and patent
clarity all favor H.264 today.

### NG5. Multi-user concurrent sessions on the same desktop
A single OpenRD session controls a single virtual or physical desktop.
Multi-user "shared screen" use cases (collaborative editing, conferencing)
are explicitly out of scope. The protocol does support a single desktop
being viewed by multiple authenticated observers, but only one has input
control at a time.

### NG6. Server-side application virtualization
OpenRD does not virtualize individual applications (no RemoteApp / SeamlessRDP
equivalent in v0). One session, one full desktop.

### NG7. Built-in directory or identity service
OpenRD does not ship its own user directory, SSO, or identity provider.
It defines a pluggable authentication interface (token bearer, mTLS,
optional OIDC integration), but does not specify how users are managed.

### NG8. UDP hole-punching and relay infrastructure in v0
NAT traversal, STUN/TURN-style relays, and rendezvous services are
out of scope for v0. v0 assumes the client can reach the server directly
(via LAN, VPN, or port-forward). A relay sub-protocol may be added later.

### NG9. Backwards compatibility with v0
v0 is explicitly a learning release. Breaking changes are expected between
v0 and v1. Long-term backwards compatibility begins at v1.

## How to use this document

When proposing a feature or change, cite the goal it advances. When
rejecting a feature, cite the non-goal it conflicts with. If neither
applies, the document is incomplete — update it.

## Open questions

- Should multi-monitor be a v0 goal or a v1 feature? Currently leaning v1.
- Should the protocol include a built-in chat channel for support
  scenarios, or is that a client-side concern? Currently leaning client-side.
- Smart-card and USB redirection: v1 or v2? Currently leaning v2.
