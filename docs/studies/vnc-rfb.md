# Case Study — VNC / RFB

> Source material: RFC 6143 (The Remote Framebuffer Protocol); TigerVNC,
> TightVNC, TurboVNC, RealVNC source.

## Overview

VNC (Virtual Network Computing) is built on the **Remote Framebuffer
Protocol (RFB)**, originally designed at the Olivetti & Oracle Research
Lab in the late 1990s and standardized as RFC 6143 in 2011. It is the
oldest widely-used open remote-desktop protocol and the spiritual ancestor
of every screen-sharing tool that does not descend from RDP.

RFB's defining decision is that it has almost no semantic understanding of
what is on the screen. The server presents a **framebuffer** — a 2D array
of pixels — and the protocol carries rectangle updates from that
framebuffer to the client. Input is just keyboard events plus mouse
position + button mask. That's the entire protocol.

## Wire format / transport

Pure **TCP, no UDP**. Default port 5900 + display number.

Connection sequence:

```
1.  ProtocolVersion handshake     (12 bytes: "RFB 003.008\n")
2.  Security types offered        (server lists, client picks)
3.  Authentication                (per chosen security type)
4.  SecurityResult                (OK or failure)
5.  ClientInit                    (shared flag)
6.  ServerInit                    (width, height, pixel format, name)
7.  Steady state: client sends messages, server sends messages.
```

Steady-state message types:

- **Client → server:** SetPixelFormat, SetEncodings, FramebufferUpdateRequest,
  KeyEvent, PointerEvent, ClientCutText.
- **Server → client:** FramebufferUpdate (with rectangles), SetColourMapEntries,
  Bell, ServerCutText.

The framebuffer update model is **pull-based**: the client requests an
update for a region, the server responds with rectangles. Modern servers
do continuous push but the protocol fundamentally treats updates as
request/response.

## Channel model

There isn't one. RFB has *one* TCP stream and multiplexes everything
inline. Clipboard (`ClientCutText` / `ServerCutText`), input, framebuffer
updates, and bell events all share the same stream. There is no concept
of a "file channel" — file transfer is a non-standard extension in some
implementations (TightVNC's `TightFileTransfer`, RealVNC's commercial
extension).

## Security model

Notoriously weak in the base spec, often tunneled.

- **VNC Authentication** — DES-based challenge/response with a password
  capped at **8 characters**. The DES key derivation strips bits, making
  brute force trivial. Deprecated, still widely deployed.
- **No encryption** in RFC 6143 itself. Implementations add TLS
  separately (`VeNCrypt` is a common extension that carries RFB inside TLS).
- **Many deployments tunnel VNC over SSH** as the actual security boundary.

## Compression / encodings

RFB defines **pixel encodings** which are also the compression scheme:

- **Raw** — pixels as-is, fallback when nothing else negotiated.
- **CopyRect** — "this rectangle is a copy of that one on the existing
  framebuffer" — cheap for scrolls and window drags.
- **RRE** (Rise-and-Run-length Encoding) — coarse RLE.
- **Hextile** — 16x16 tiles, per-tile encoding choice.
- **TRLE / ZRLE** — Tile RLE; ZRLE adds zlib.
- **Tight** — TightVNC's encoding, JPEG for photo-like tiles + zlib for
  the rest. Generally best quality/bandwidth tradeoff for desktops.
- **Tight + libjpeg-turbo (TurboVNC)** — same Tight encoding, JIT'd JPEG.
  Surprisingly competitive on modern hardware.
- **H.264 / H.265 encodings** — non-standard, vendor-specific extensions.

## What's good

1. **Conceptual simplicity.** You can implement a usable RFB server in a
   weekend. The spec is short, the state machine is shallow, and the
   message types are obvious.
2. **Format negotiation.** `SetEncodings` lets the client tell the server
   which encodings it understands, in preference order. Server picks
   per-rectangle. This is exactly the pattern OpenRD should use for
   per-channel codec negotiation.
3. **Pixel-format agnostic.** Server and client negotiate bits-per-pixel,
   channel layout, and endianness — a single server can serve 8-bit,
   16-bit, and 32-bit clients without changes.
4. **CopyRect** is genuinely clever. Window drags become zero-payload
   updates.
5. **The protocol has lasted 25+ years.** Whatever its faults, the basic
   shape works.

## What's bad

1. **One TCP stream for everything.** Head-of-line blocking: a slow
   clipboard transfer blocks input. A big framebuffer update blocks
   everything else.
2. **No file transfer in the spec.** Every implementation invented its
   own incompatible extension.
3. **Security in the base spec is broken.** 8-char DES auth in 2026 is
   not acceptable. The fact that TLS is a layered extension rather than
   mandatory means many deployments run cleartext.
4. **Pull-based updates** add a round-trip on every frame. Modern
   implementations work around this with "continuous updates" extension
   but the fundamental request/response model wastes time.
5. **No session identity.** A TCP reconnect is a fresh session; there's
   no resumption.
6. **Pixel format negotiation is complex** and rarely useful in 2026 when
   essentially every client is 32-bit RGBA.
7. **Audio is a non-standard extension.** Every server does it
   differently, or not at all.

## What OpenRD should copy

1. **Encoding negotiation pattern.** `SetEncodings`-style preference lists
   for codecs (per-channel: video codec, audio codec, clipboard MIME
   types).
2. **CopyRect equivalent.** Server-side detection of repeated content;
   send a "copy from offset (x1,y1) to offset (x2,y2)" message instead
   of pixels. Bitmap cache (from RDP) and CopyRect (from VNC) together
   are very powerful for productivity workloads.
3. **Short, readable spec.** RFC 6143 is ~70 pages and a competent
   programmer can implement it from the document alone. OpenRD v0 should
   target similar readability.

## What OpenRD should do differently

1. **Multi-channel, not single-stream.** Independent channels for
   input, display, clipboard, audio, files, control.
2. **Crypto mandatory.** No cleartext, no DES-with-8-byte-passwords.
   TLS 1.3 from byte one.
3. **Push, not pull, for the display channel.** Server streams; client
   acks/requests keyframes only as needed.
4. **File transfer in the core spec, not an extension.**
5. **Stable session identity** across reconnects.
6. **Fixed pixel format** (32-bit BGRA or YUV4:2:0 for encoded video).
   No client-side pixel-format negotiation gymnastics.

## References

- RFC 6143 — The Remote Framebuffer Protocol
- TigerVNC: github.com/TigerVNC/tigervnc
- TurboVNC: github.com/TurboVNC/turbovnc
- The original Olivetti paper: "Virtual Network Computing", Richardson et
  al., 1998
