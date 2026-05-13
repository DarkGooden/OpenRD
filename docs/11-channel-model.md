# 11 — Channel Model

> Status: Draft v0.1
> Last updated: 2026-05-13

A **channel** in OpenRD is a logical, typed, named, independently-flow-
controlled stream of data between client and server. Channels are
multiplexed onto a single QUIC connection; each channel occupies one
or more QUIC streams.

This document enumerates the v0 channels, their direction,
reliability requirements, allowed payload sizes, and their lifecycle.

## Why channels (and not "just one stream")

Reasons established by case studies and the threat model:

- **Independent forward progress.** A 600 MB file transfer must not
  block a keystroke. QUIC streams give us this for free.
- **Per-channel flow control.** The receiver can throttle one channel
  without affecting others.
- **Typed payloads.** Each channel has its own message schema. Cross-
  channel confusion (T-14) is prevented by per-stream typing.
- **Independent failure.** A misbehaving file transfer can be torn
  down without dropping the session.

## Channel ID space

Channels are identified by a 16-bit unsigned **channel kind** (the
type) and, for the duration of a single session, a 32-bit
**channel instance ID** (assigned by whichever side opens the channel).

Kinds are constants defined by the spec; instance IDs are runtime
allocations.

| Kind value | Name             | Direction          | Cardinality       |
|------------|------------------|--------------------|--------------------|
| 0x0001     | Control          | bidirectional      | exactly one        |
| 0x0002     | Display          | server → client    | one (v0)           |
| 0x0003     | Cursor           | server → client    | one (v0)           |
| 0x0004     | Input            | client → server    | one                |
| 0x0005     | Clipboard        | bidirectional      | one                |
| 0x0006     | File             | bidirectional      | many (one per transfer) |
| 0x0007     | Audio (playback) | server → client    | one                |
| 0x0008     | Stats            | bidirectional      | one (optional)     |
| 0x0009     | Chat             | bidirectional      | one (optional)     |
| 0x0100–0x01FF | Reserved      | —                  | for v1+ standard channels |
| 0x8000–0xFFFF | Vendor        | —                  | for vendor-private extensions |

Kinds 0x000A–0x00FF are reserved for future v0 amendments. Vendor
extensions in the 0x8000–0xFFFF range must not conflict with the
standard space and must be ignorable by peers that do not understand
them.

---

## The Control channel (0x0001)

> **Mandatory.** Exactly one Control channel per session. It is the
> first channel opened, and the last one closed. Closing it ends the
> session.

The Control channel carries:

- Capability negotiation (see [`22-capability-negotiation.md`](22-capability-negotiation.md)).
- Channel open / close requests.
- Authentication state changes (e.g., consent prompts for permission
  elevation).
- Session events (resolution change, permission level change,
  keepalive).
- Structured errors.

**Reliability:** ordered, reliable (QUIC bidirectional stream).
**Payload:** length-prefixed CBOR messages, max 1 MB per message.
**Cardinality:** exactly one per session.

---

## The Display channel (0x0002)

The Display channel carries the encoded video stream of the remote
desktop.

- **Codec:** H.264 Baseline / Constrained Baseline in v0. Future
  codecs negotiated via capabilities.
- **Frame model:** IDR keyframes + P-frames. No B-frames. Small GOP
  (default 60 frames). Server-side encoder MUST honor
  `tune=zerolatency`-equivalent settings.
- **Frame slicing:** each frame is split into 4–16 slices, each sent
  as a separate QUIC message. A single packet loss damages at most
  one slice.
- **IDR on demand:** the client can send a `RequestKeyframe` on the
  Control channel; the server emits an IDR at the next encode
  opportunity (target < 1 frame interval).

**Reliability:** unreliable-preferred. v0 uses a *reliable* QUIC stream
for simplicity; v1 may add a datagram-based mode for stricter latency.
**Direction:** server → client (unidirectional QUIC stream).
**Cardinality:** exactly one in v0. Multi-monitor support is v1+.

---

## The Cursor channel (0x0003)

Cursor position and shape are carried on a **separate channel** from
Display so that cursor movement is not gated by video frame intervals.

- Frequency: server may emit up to 240 cursor updates per second.
- Each message is small (≤ 128 B for position-only; up to a few KB
  for shape changes).
- Client renders the cursor as a sprite client-side; the server
  does not draw the cursor into the framebuffer (or, where it must,
  emits a "cursor is in-frame" hint so the client suppresses the
  client-side overlay).

**Reliability:** unreliable preferred. Latest cursor position
overrides older ones; if a position update is lost, the next one
will arrive within a few ms.
**Direction:** server → client.

---

## The Input channel (0x0004)

The Input channel carries keyboard, mouse, and touch events from
client to server.

Message types:

- `KeyEvent { keysym, scancode, modifiers, down }`
- `PointerMove { x, y, abs_or_rel }`
- `PointerButton { button, down }`
- `PointerWheel { dx, dy, mode }`
- `TouchEvent { id, x, y, pressure, phase }`
- `SyntheticBatch { events[] }` — paste-as-typing fallback.

**Reliability:** ordered, reliable. Input loss would silently corrupt
the user's state.
**Direction:** client → server.
**Latency target:** ≤ 5 ms server-side processing per event.

---

## The Clipboard channel (0x0005)

Bidirectional clipboard transfer. **Paste-pull model only** — the
side that wants to paste asks the other side for current clipboard
content; the other side sends. There is no "I just copied something"
push.

Message types:

- `OfferTypes { types[] }` — sent when the local clipboard contents
  change, listing available MIME types but NOT the content.
- `RequestContent { type }` — request a specific MIME type.
- `Content { type, bytes }` — response with the data.

Supported types in v0: `text/plain;charset=utf-8`, `text/html`,
`text/rtf`, `image/png`, `image/jpeg`.

**Reliability:** ordered, reliable.
**Direction:** bidirectional.
**Size limit:** 64 MB per content payload. Larger transfers must use
the File channel.

---

## The File channel (0x0006)

The File channel handles explicit file and directory transfers.
Unlike RDP's drive redirection, OpenRD's File channel is a transfer
protocol, not a remote filesystem.

Many File channels can be open simultaneously — one per active
transfer. Each transfer is identified by its instance ID.

Per-transfer message types:

- `StartTransfer { transfer_id, direction, manifest, chunk_size }`
- `Manifest { paths[], sizes[], permissions[], root_hash }`
- `Chunk { transfer_id, file_index, chunk_index, bytes }`
- `AckChunk { transfer_id, file_index, chunk_index, status }`
- `EndTransfer { transfer_id, status }`

**Reliability:** ordered, reliable (per stream).
**Direction:** bidirectional (initiator chooses).
**Size:** per-file at least 16 GB. Total transfer manifest at least
1 GB of entries.
**Concurrency:** multiple File channels OK; receiver may rate-limit.

See [`studies/file-transfer.md`](studies/file-transfer.md) for the
design rationale.

---

## The Audio channel (0x0007)

Server-to-client audio playback.

- **Codec:** Opus, 48 kHz, mono or stereo.
- **Default bitrate:** 64 kbps stereo / 32 kbps mono.
- **Packetization:** 20 ms frames, one Opus packet per QUIC
  message.

**Reliability:** unreliable preferred (loss tolerable, late is
useless).
**Direction:** server → client.

Client-microphone audio (server-bound) is **not in v0**.

---

## The Chat channel (0x0009)

The Chat channel carries real-time text chat between the two human
operators of a session — typically a support technician and an
end user (UC-2). Optional; only open it if both peers advertise
support.

Message types:

- `ChatMessage { id, sender, body, ts_ms }` — UTF-8 text message
- `TypingIndicator { sender, state }` — `state ∈ {start, stop}`
- `ChatAttachment { id, sender, mime, bytes }` — inline attachment
  up to 1 MiB. Larger attachments MUST use the File channel.

v0 deliberately omits: read receipts, edit/delete, threading,
reactions. These are v1+ considerations.

**Reliability:** ordered, reliable.
**Direction:** bidirectional.
**Cardinality:** zero or one.

---

## The Stats channel (0x0008)

Optional. Carries periodic telemetry from each side to the other:

- RTT, packet loss, retransmits.
- Encode time, decode time, queue depths.
- Per-channel throughput.
- Buffer underruns / dropped frames.

**Reliability:** unreliable.
**Direction:** bidirectional.
**Cardinality:** zero or one. If absent, peers compute their own
stats locally.

The Stats channel exists primarily for debugging and adaptive
rate control. A v0 implementation MAY omit it without losing
conformance.

---

## Channel-open authorization

Not every channel can be opened by either side, and not every
permission level can open every channel. The Control channel is
opened implicitly at session start; everything else requires an
explicit `OpenChannel` and is subject to authorization:

| Channel    | Server may open | Client may open | Requires permission |
|------------|-----------------|-----------------|----------------------|
| Control    | (implicit)      | (implicit)      | n/a                  |
| Display    | Yes             | No              | view-only or higher  |
| Cursor     | Yes             | No              | view-only or higher  |
| Input      | No              | Yes             | interactive          |
| Clipboard  | Yes             | Yes             | interactive          |
| File       | Yes             | Yes             | interactive          |
| Audio      | Yes             | No              | view-only or higher  |
| Stats      | Yes             | Yes             | view-only or higher  |
| Chat       | Yes             | Yes             | view-only or higher  |

A server MUST refuse an OpenChannel for which the requesting side
lacks permission, with a structured error (T-13 mitigation).

---

## Resolved questions

All open questions for v0 are resolved — see [`decisions.md`](decisions.md):
- D8: Multi-monitor uses N Display channels (v1 intent).
- D9: Stats channel is protocol metrics only.
- D18: Vendor extension space is unmanaged in v0.
