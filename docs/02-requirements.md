# 02 — Requirements

> Status: Draft v0.1
> Last updated: 2026-05-13

This document distills [`use-cases`](01-use-cases.md) into testable
requirements. Each requirement has a stable identifier so it can be cited
elsewhere in the spec.

Requirements are grouped into **Functional (F-)**, **Non-Functional (NF-)**,
and **Constraints (C-)**.

---

## Functional requirements

### F-1. Channels

The protocol MUST support the following logically independent channels.
Channels run concurrently and do not block each other.

| ID    | Channel    | Direction      | v0?   |
|-------|------------|----------------|-------|
| F-1.1 | Control    | Bidirectional  | Yes   |
| F-1.2 | Display    | Server → Client| Yes   |
| F-1.3 | Input      | Client → Server| Yes   |
| F-1.4 | Clipboard  | Bidirectional  | Yes   |
| F-1.5 | File       | Bidirectional  | Yes   |
| F-1.6 | Audio (server playback) | Server → Client | Yes |
| F-1.7 | Audio (client mic)      | Client → Server | v1+ |
| F-1.8 | USB redirection         | Bidirectional   | v2+ |
| F-1.9 | Printer redirection     | Bidirectional   | v2+ |

### F-2. Input

- F-2.1. The Input channel MUST carry keyboard events with full Unicode
  semantics (not just US ASCII).
- F-2.2. Modifier keys (Shift, Ctrl, Alt, Meta/Super/Cmd, AltGr) MUST be
  representable independently of the key event itself.
- F-2.3. Mouse events MUST support absolute and relative coordinates,
  multi-button (at least 5 buttons), scroll wheel (vertical and horizontal),
  and high-resolution scroll (fractional units).
- F-2.4. The protocol MUST support touch events for mobile clients: at
  least one-point and two-point gestures, with pressure as an optional
  field.
- F-2.5. The client SHOULD be able to send a sequence of synthetic input
  events as an atomic batch (for paste-as-typing fallbacks).

### F-3. Clipboard

- F-3.1. Clipboard MUST support `text/plain` (UTF-8).
- F-3.2. Clipboard MUST support `image/png` and `image/jpeg`.
- F-3.3. Clipboard SHOULD support `text/html` and `text/rtf`.
- F-3.4. Clipboard payloads up to 64 MB MUST be supported. Beyond 64 MB,
  the protocol SHOULD reuse the File channel.
- F-3.5. The clipboard transfer MUST be initiated by an explicit paste
  on the receiving side, not pushed eagerly. (Privacy: copying something
  locally should not leak it to the remote side until the remote pastes.)

### F-4. File transfer

- F-4.1. File transfer MUST support single files of at least 16 GB.
- F-4.2. File transfer MUST support directory trees (recursive).
- F-4.3. File transfer MUST report progress (bytes transferred, ETA).
- F-4.4. File transfer MUST be resumable after a connection drop of up
  to NF-3.5 seconds.
- F-4.5. File transfer SHOULD support integrity verification
  (SHA-256 hash) and reject corrupt payloads.
- F-4.6. File transfer MUST NOT block the Input or Display channels.

### F-5. Display

- F-5.1. The Display channel MUST carry encoded video frames; v0 MUST
  use H.264 Baseline / Constrained Baseline.
- F-5.2. The server MUST be able to deliver at least 1080p @ 30fps with
  CPU-only encode on commodity hardware (e.g., a 2020-era 4-core x86_64
  CPU).
- F-5.3. The server MUST support dynamic resolution changes (client
  resizes the viewport) without dropping the session.
- F-5.4. The server SHOULD send a keyframe on demand (client request),
  for fast resync after packet loss or seek.

### F-6. Audio

- F-6.1. Server playback audio MUST use Opus at 48 kHz, mono or stereo.
- F-6.2. Default audio bitrate target: 64 kbps stereo, 32 kbps mono.
- F-6.3. The audio channel MUST be muteable client-side without
  affecting other channels.

### F-7. Authentication & session

- F-7.1. The server MUST authenticate the client using one of:
  bearer token, TLS client certificate, or short-lived signed invitation
  token.
- F-7.2. The client MUST verify the server's TLS certificate against a
  pinned key or a configured CA.
- F-7.3. The session MUST support resumption across short connection
  losses (see NF-3.5).
- F-7.4. The server MUST be able to revoke a session at any time;
  revocation MUST take effect within 1 second.
- F-7.5. The protocol MUST distinguish "view-only" and "interactive"
  permission levels, and support runtime upgrade with an explicit consent
  prompt routed through the Control channel.

### F-8. Capability negotiation

- F-8.1. Client and server MUST exchange capability descriptors before
  any non-Control channel opens.
- F-8.2. Both peers MUST gracefully handle unknown capabilities (ignore
  rather than error).
- F-8.3. A version mismatch MUST produce a clear, machine-readable
  error rather than a silent stall.

---

## Non-functional requirements

### NF-1. Latency

- NF-1.1. **Input-to-pixel round trip ≤ 30 ms** on a 1 Gbps LAN with
  < 1 ms RTT, at 1080p60, CPU-only encode.
- NF-1.2. **Input-to-pixel round trip ≤ 100 ms** on a 50 Mbps WAN with
  20 ms RTT, at 1080p30.
- NF-1.3. **Connection establishment ≤ 2 s** including TLS handshake,
  authentication, and first frame, on a 50 Mbps link.

### NF-2. Throughput and bandwidth

- NF-2.1. The protocol MUST function (degraded but usable) on a 2 Mbps
  symmetric link.
- NF-2.2. The protocol MUST scale to a 100 Mbps link without protocol
  overhead exceeding 5 % of total bytes (measured as non-payload bytes
  on the wire).

### NF-3. Reliability

- NF-3.1. The session MUST survive a transient packet loss of 5 % on
  the underlying network for 10 s without disconnecting.
- NF-3.2. The session MUST survive a brief total network outage (≤ 5 s)
  via transport-level retransmit.
- NF-3.3. The session MUST survive a longer outage (≤ 60 s, configurable)
  via explicit session resumption.
- NF-3.4. After NF-3.3 expires, the session MUST be cleanly torn down
  and the client MUST require re-authentication.
- NF-3.5. The default resumption grace period is 30 s.

### NF-4. Security

- NF-4.1. All channels MUST be encrypted with TLS 1.3 (or
  QUIC-equivalent TLS 1.3).
- NF-4.2. No unencrypted fallback MUST exist in any conformant
  implementation.
- NF-4.3. Server-to-client and client-to-server authentication MUST be
  mutual or one-sided-with-strong-token; anonymous sessions are forbidden.
- NF-4.4. Session keys MUST NOT be derived from user passwords without
  a modern KDF (Argon2id or scrypt with sane parameters).
- NF-4.5. The protocol MUST resist replay attacks for at least input,
  clipboard, and control messages.

### NF-5. Portability

- NF-5.1. The wire format MUST be byte-oriented and little-endian where
  endianness applies. (TLS-style network byte order is acceptable for
  TLS-defined fields but the OpenRD layer is little-endian.)
- NF-5.2. The reference server MUST compile and run on current LTS
  releases of Ubuntu, Debian, RHEL/Rocky, and Alpine.
- NF-5.3. The reference clients MUST run on Windows 10+, macOS 12+,
  iOS 16+, Android 10+, and modern Chrome/Firefox/Safari.

### NF-6. Resource usage

- NF-6.1. Idle session memory at the server SHOULD be under 64 MB.
- NF-6.2. The web client MUST run in a single browser tab without
  requiring service workers, WebGPU, or experimental features.

### NF-7. Observability

- NF-7.1. The protocol MUST expose a Control-channel "stats" message
  with at least: round-trip time, packet loss, encode time, decode time,
  channel throughput.
- NF-7.2. The Control channel MUST carry structured error codes
  (numeric + machine-readable string).

---

## Constraints

### C-1. The author's deliberate constraints

- C-1.1. Server platform is **Linux only** for v0.
- C-1.2. Encode is **CPU-only** for v0. GPU encode is permitted but
  must never be required.
- C-1.3. v0 codec is **H.264 Baseline / Constrained Baseline only**.
- C-1.4. License is **Apache 2.0** for both spec and reference
  implementations.

### C-2. Environmental constraints

- C-2.1. Web clients are subject to browser API constraints (no raw
  sockets; WebTransport or WebSocket-over-TLS or WebRTC only).
- C-2.2. Mobile clients are subject to background-execution restrictions
  (sessions may be backgrounded; protocol must handle pause/resume).
- C-2.3. Firewalls commonly block UDP; the protocol MUST work over
  TCP-based transport as a fallback.

---

## Out of scope for v0 (see [non-goals](00-goals-and-non-goals.md))

- Multi-monitor (deferred to v1)
- Client microphone audio (deferred to v1)
- USB and printer redirection (v2)
- NAT traversal and relay infrastructure (v1)
- AV1 / H.265 / VP9 / JPEG-XL (post-v0)

---

## Traceability

Every requirement above MUST trace back to at least one use case in
[`01-use-cases.md`](01-use-cases.md). Where the trace is non-obvious, a
follow-up version of this document will add a column linking the two.
