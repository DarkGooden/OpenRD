# Case Study — Microsoft RDP

> Source material: MS-RDPBCGR, MS-RDPEGFX, MS-RDPECLIP, MS-RDPEFS public
> specifications; FreeRDP source.

## Overview

The Remote Desktop Protocol (RDP) is Microsoft's family of protocols for
remote graphical sessions. The base specification (MS-RDPBCGR) dates to the
NT 4.0 Terminal Server Edition era (1998) and has been continuously extended
since. It is the most widely deployed remote-desktop protocol in existence.

RDP is layered on a stack inherited from ITU-T multimedia conferencing
standards:

```
Application PDUs (RDP)
        |
   Security layer (Standard RDP / TLS / CredSSP/NLA)
        |
        MCS  (T.125 — multipoint channel multiplex)
        |
        X.224 (T.123 — connection-oriented transport class 0)
        |
        TPKT (RFC 1006 — TCP encapsulation)
        |
        TCP/3389
```

In RDP 8 and later, an optional **UDP side-channel** carries graphics and
audio for lower latency, falling back to TCP-only if UDP is blocked.

## Channel model

RDP uses **virtual channels** carried over MCS, which gives it true
multi-channel multiplexing. Each channel has a 64-byte ASCII name (e.g.
`CLIPRDR`, `RDPDR`, `RDPSND`, `DRDYNVC`) and a numeric channel ID assigned
at session setup.

**Static virtual channels** are declared at connection time and exist for
the life of the session. **Dynamic virtual channels (DVC)** are opened on
demand inside a meta-channel called `DRDYNVC`, and this is where most
modern extensions live (RemoteFX, graphics pipeline, USB redirection).

Key channels:

- **I/O channel** (no name; multiplexed inside MCS) — display PDUs and
  input events.
- **CLIPRDR** — clipboard.
- **RDPDR** — drive, printer, smart card, port redirection.
- **RDPSND** / **AUDIO_INPUT** — audio playback / capture.
- **MS_T120** / **DRDYNVC** — control / dynamic channel manager.

## Security model

Three modes, in chronological order of introduction:

1. **Standard RDP Security** — RC4 with a custom key exchange. Broken,
   deprecated. Modern Windows refuses it by default.
2. **TLS/SSL** — RDP PDUs wrapped in TLS after the X.224 handshake.
3. **CredSSP / NLA (Network Level Authentication)** — TLS plus SPNEGO
   (Kerberos or NTLM) auth performed *before* the server allocates a
   session. This is the modern default. NLA matters because it prevents
   the server from spinning up a winlogon screen for an unauthenticated
   attacker.

## Compression / graphics

RDP's graphics pipeline has been rewritten several times:

- **Original (1998)** — Server sent **GDI primitives** (DrawRect, BitBlt,
  glyph cache). Tiny on the wire for text-heavy desktops; the client
  rendered locally.
- **RemoteFX (Windows 7 SP1)** — Tile-based bitmap codec, hardware-accelerated
  on the server when available, plus USB-over-RDP and progressive image
  encoding.
- **RDP 8+ / Graphics Pipeline (MS-RDPEGFX)** — H.264/AVC 444 for full-screen
  video, plus a progressive RemoteFX-style codec for static regions. Tiles
  are classified and routed to the appropriate codec.
- **Bitmap cache & glyph cache** — Persistent client-side caches for
  recurring bitmaps and font glyphs, keyed by ID. Huge win for text workloads:
  a paragraph re-rendered after a scroll resends only cache references, not
  pixels.

## What's good

- **Multi-channel multiplexing is correct.** Independent channels for
  display, input, clipboard, file, audio. The model has been validated by
  ~25 years of deployment.
- **Bitmap cache and glyph cache** are exactly right for productivity
  workloads. Modern protocols that skip this (gaming-focused ones) waste
  bandwidth on text-heavy desktops.
- **NLA's "auth before session"** model is good security hygiene.
- **Capability negotiation** is comprehensive — every feature is gated
  behind a capability flag.
- **UDP fallback to TCP** acknowledges that UDP is sometimes blocked, which
  is a real-world constraint OpenRD must also handle.

## What's bad

- **Stack depth.** TPKT → X.224 → MCS → Security → RDP is at least four
  framing layers before payload. Each adds parsing complexity. MCS in
  particular is overkill for a single-client session.
- **ASN.1 PER encoding** in connection setup. ASN.1 BER/PER is fiddly
  enough that implementations rarely agree on edge cases.
- **In-band capability negotiation, in-band auth, in-band crypto.** Modern
  protocols separate these concerns; RDP mixes them across layers.
- **Bit-packed PDU formats** with mandatory padding and per-flag conditional
  fields. Implementing a parser is unpleasant and bug-prone.
- **Compression negotiation is a maze.** Legacy MPPC, then 64K, then RDP 8,
  then EGFX. A v0 implementation has to support a startling amount of
  history to interoperate.
- **Clipboard model leaks data eagerly.** The server can pull the client's
  clipboard contents without the user pasting. This is a real privacy
  concern.
- **No proper session resumption.** A dropped TCP connection means a new
  handshake; the server retains the desktop briefly via "reconnect cookies"
  but the model is weak.

## What OpenRD should copy

1. **Independent typed channels with names and IDs**, including a
   meta-channel for dynamic channel management.
2. **Bitmap cache + glyph cache** — keep this for productivity workloads
   even though we use H.264 by default; cache references in the Control
   channel can override video-encoded regions to keep text crisp.
3. **Auth-before-session** model (NLA's spirit). Don't allocate desktop
   resources for an unauthenticated peer.
4. **Capability negotiation** as a first-class step.
5. **UDP-first with TCP fallback**.

## What OpenRD should do differently

1. **One framing layer**, not five. TLS 1.3 / QUIC handles framing,
   crypto, multiplexing, and reliability — we layer OpenRD frames directly
   on QUIC streams.
2. **No ASN.1.** Length-prefixed binary with explicit field tags. CBOR
   or a custom TLV scheme, not ASN.1 PER.
3. **Clipboard is paste-pull, never copy-push.** See [`threat model T-7`](../03-threat-model.md).
4. **One codec in v0** (H.264 Baseline). No tile-classification dance.
   Add codecs in later versions via capability negotiation, not at
   launch.
5. **Session ID independent of transport connection** — a session lives
   across reconnects.
6. **Endianness is little-endian, period.** No mixed endianness, no
   network-order legacy fields.

## References

- MS-RDPBCGR — Remote Desktop Protocol: Basic Connectivity and Graphics
  Remoting
- MS-RDPEGFX — Remote Desktop Protocol: Graphics Pipeline Extension
- MS-RDPECLIP — Remote Desktop Protocol: Clipboard Virtual Channel Extension
- MS-RDPEFS — Remote Desktop Protocol: File System Virtual Channel Extension
- FreeRDP source: github.com/FreeRDP/FreeRDP
