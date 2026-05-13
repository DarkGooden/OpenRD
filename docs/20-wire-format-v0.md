# 20 — Wire Format v0

> Status: Draft v0.1 — subject to change as the reference implementation
> shakes out issues.
> Last updated: 2026-05-13

This document specifies the byte-level layout of every OpenRD v0
message as it appears on a QUIC stream. It is normative; conformant
implementations MUST match it.

## Conventions

- **Endianness:** All multi-byte integers are **little-endian**
  unless explicitly noted (TLS-defined fields inside the QUIC
  handshake remain network byte order; OpenRD's own bytes are
  little-endian).
- **CBOR profile:** Control-channel messages use CBOR
  **Preferred Serialization** (RFC 8949 §4.1) by default. Signed
  structures (resumption tokens, invitation tokens) MUST use
  **Deterministic Encoding** (RFC 8949 §4.2) so the receiver can
  re-hash the bytes for signature verification.
- **Notation:** Fields are described as `name : type` followed by
  a description. Types are:
  - `u8`, `u16`, `u32`, `u64` — unsigned little-endian integers
  - `i32`, `i64` — signed little-endian integers (two's complement)
  - `bytes[N]` — fixed N-byte octet string
  - `bytes<N>` — variable-length octet string with `N` as the
    length-field width: `bytes<u32>` means a 4-byte length followed
    by that many bytes
  - `cbor` — CBOR-encoded value (RFC 8949)
- **Padding:** None. There are no alignment requirements.
- **Reserved fields:** MUST be zero on transmission; MUST be ignored
  (not checked) on receipt.
- **Version field:** Every length-prefixed top-level frame begins
  with an explicit `version : u8`. v0 = `0x00`.

## Stream layout

Each channel uses one or more QUIC streams. Within a stream, data
is a sequence of **frames**:

```
+--------+--------+--------------+----------+
| ver:u8 | type:u8| length:u32   | payload  |
+--------+--------+--------------+----------+
       ↑ 1 byte ↑ 1 byte ↑ 4 bytes ↑ length bytes
```

- `version`: protocol version of this frame. v0 = `0x00`.
- `type`: frame type, channel-specific.
- `length`: payload size in bytes. Hard cap: 16 MiB
  (`0x01_00_00_00`). Frames larger than this are an error.
- `payload`: type-specific bytes, as defined below.

All channels share this outer envelope. The `type` namespace is
per-channel.

## QUIC stream allocation

Streams are assigned at channel-open time. The Control channel uses
QUIC bidirectional stream ID `0` (the first client-initiated
bidirectional stream). All other channels are allocated by the side
that opens them, using QUIC's stream-ID conventions:

- Client-initiated bidirectional: 0, 4, 8, ...
- Server-initiated bidirectional: 1, 5, 9, ...
- Client-initiated unidirectional: 2, 6, 10, ...
- Server-initiated unidirectional: 3, 7, 11, ...

The `OpenChannel` Control message tells the peer which stream ID
will carry the channel.

---

## Control channel frames (kind 0x0001)

### Frame types

| Type | Name                  | Direction | Description                              |
|------|-----------------------|-----------|------------------------------------------|
| 0x01 | ClientHello           | C → S     | Initial capability advertisement         |
| 0x02 | ServerHello           | S → C     | Server capability advertisement + session info |
| 0x03 | AuthRequest           | C → S     | Credential presentation                  |
| 0x04 | AuthChallenge         | S → C     | Optional server challenge (PIN, mTLS, etc.)|
| 0x05 | AuthResult            | S → C     | Outcome of authentication                |
| 0x06 | OpenChannel           | bidir     | Request to open a new channel            |
| 0x07 | OpenChannelAck        | bidir     | Acknowledgement / refusal                |
| 0x08 | CloseChannel          | bidir     | Tear down a channel                      |
| 0x09 | ConsentRequest        | S → C     | Ask user to approve an action (e.g. elevation)|
| 0x0A | ConsentResponse       | C → S     | User's answer                            |
| 0x0B | SessionEvent          | bidir     | Resolution change, pause, etc.           |
| 0x0C | Error                 | bidir     | Structured error                         |
| 0x0D | RequestKeyframe       | C → S     | Display channel keyframe-on-demand       |
| 0x0E | Ping                  | bidir     | Keepalive                                |
| 0x0F | Pong                  | bidir     | Keepalive response                       |
| 0x10 | Stats                 | bidir     | Mirror of Stats channel (if no Stats ch.)|
| 0x11 | SessionResume         | C → S     | Resume an existing session               |
| 0x12 | SessionResumed        | S → C     | Resumption successful                    |

Control payloads are encoded as **CBOR maps** keyed by short integer
keys. CBOR rather than JSON for size and parser robustness.

### ClientHello (type 0x01)

```
CBOR map:
  1 (protocol_version) : uint   ; MUST be 0
  2 (client_name)      : tstr   ; e.g. "openrd-rust/0.1.0"
  3 (capabilities)     : map    ; see capability negotiation doc
  4 (session_id_hint)  : bstr   ; optional, for resumption (16 bytes)
```

### ServerHello (type 0x02)

```
CBOR map:
  1 (protocol_version) : uint
  2 (server_name)      : tstr
  3 (capabilities)     : map
  4 (session_id)       : bstr   ; 16 bytes, newly allocated unless resuming
  5 (server_time)      : uint   ; unix epoch seconds (for clock-skew detection)
```

### AuthRequest (type 0x03)

```
CBOR map:
  1 (method)           : tstr   ; "token" | "pin" | "mtls" | "oidc"
  2 (credential)       : bstr   ; opaque; meaning depends on method
  3 (identity_claim)   : tstr   ; optional, e.g. username
```

### AuthChallenge (type 0x04)

```
CBOR map:
  1 (challenge_kind)   : tstr   ; "pin_display" | "totp" | ...
  2 (challenge_data)   : any    ; method-specific
```

### AuthResult (type 0x05)

```
CBOR map:
  1 (status)           : uint   ; 0 = OK, nonzero = failure code (see Error codes)
  2 (permission)       : tstr   ; "view-only" | "interactive"
  3 (identity)         : tstr   ; authenticated identity (for client display)
  4 (resumption_token) : bstr   ; optional, used in later resumption
  5 (resumption_ttl_s) : uint   ; lifetime of resumption window (default 30)
```

### OpenChannel (type 0x06)

```
CBOR map:
  1 (channel_kind)     : uint   ; e.g. 0x0002 for Display
  2 (channel_id)       : uint   ; instance ID, unique within session
  3 (stream_id)        : uint   ; QUIC stream ID that will carry this channel
  4 (params)           : map    ; channel-kind-specific (e.g. preferred codec)
```

### OpenChannelAck (type 0x07)

```
CBOR map:
  1 (channel_id)       : uint   ; echoes the request
  2 (status)           : uint   ; 0 = accepted, nonzero = error code
  3 (negotiated)       : map    ; final parameters (codec, dims, etc.)
```

### CloseChannel (type 0x08)

```
CBOR map:
  1 (channel_id)       : uint
  2 (reason_code)      : uint   ; 0 = normal, nonzero = error
  3 (reason_text)      : tstr   ; optional human-readable
```

### ConsentRequest (type 0x09) and ConsentResponse (type 0x0A)

```
ConsentRequest:
  1 (consent_id)       : uint
  2 (action)           : tstr   ; "elevate_to_interactive" | "accept_file" | ...
  3 (details)          : map    ; action-specific
  4 (timeout_ms)       : uint

ConsentResponse:
  1 (consent_id)       : uint
  2 (granted)          : bool
  3 (reason)           : tstr   ; optional
```

### SessionEvent (type 0x0B)

```
CBOR map:
  1 (event)            : tstr   ; "resolution_change" | "permission_change" | "paused" | ...
  2 (data)             : map    ; event-specific
```

### Error (type 0x0C)

```
CBOR map:
  1 (code)             : uint   ; numeric error code (see table below)
  2 (text)             : tstr   ; human-readable
  3 (channel_id)       : uint   ; optional, scopes the error to a channel
  4 (fatal)            : bool   ; true if the session must terminate
```

### RequestKeyframe (type 0x0D)

```
CBOR map:
  1 (display_channel_id) : uint
```

### Ping / Pong (types 0x0E, 0x0F)

```
CBOR map:
  1 (nonce)            : bstr   ; echoed in Pong
  2 (sent_at_ms)       : uint   ; sender's monotonic time
```

### SessionResume (type 0x11)

Sent before AuthRequest when reconnecting within the resumption
window. If accepted, AuthRequest is skipped.

```
CBOR map:
  1 (session_id)       : bstr   ; 16 bytes
  2 (resumption_token) : bstr   ; from prior AuthResult
```

### SessionResumed (type 0x12)

```
CBOR map:
  1 (status)           : uint   ; 0 = OK
  2 (new_resumption_token) : bstr  ; for the next resume
```

### Error codes

| Code      | Name                          |
|-----------|-------------------------------|
| 0x0000    | OK                            |
| 0x0001    | INVALID_FRAME                 |
| 0x0002    | UNSUPPORTED_VERSION           |
| 0x0003    | UNAUTHENTICATED               |
| 0x0004    | AUTH_FAILED                   |
| 0x0005    | PERMISSION_DENIED             |
| 0x0006    | UNKNOWN_CHANNEL_KIND          |
| 0x0007    | CHANNEL_LIMIT_EXCEEDED        |
| 0x0008    | RESOURCE_EXHAUSTED            |
| 0x0009    | RATE_LIMITED                  |
| 0x000A    | SESSION_EXPIRED               |
| 0x000B    | RESUMPTION_REJECTED           |
| 0x000C    | INTERNAL                      |
| 0x000D    | NOT_IMPLEMENTED               |
| 0x000E    | CONSENT_DENIED                |
| 0x000F    | INVALID_PARAMETER             |
| 0xFFFF    | VENDOR_DEFINED                |

---

## Display channel frames (kind 0x0002)

The Display channel carries encoded H.264 NAL units sliced for
loss tolerance.

| Type | Name             | Description                                            |
|------|------------------|--------------------------------------------------------|
| 0x01 | FrameHeader      | Metadata for a frame                                   |
| 0x02 | FrameSlice       | One slice of the current frame                         |
| 0x03 | FrameEnd         | Marks completion of a frame                            |
| 0x04 | StreamParameters | SPS/PPS update (sent before any frame and on changes)  |

### StreamParameters (type 0x04)

```
+--------+--------+--------------+
| codec  | width  | height       |
| :u8    | :u16   | :u16         |
+--------+--------+--------------+
| sps_length:u16 | sps_bytes...  |
+--------+--------+--------------+
| pps_length:u16 | pps_bytes...  |
+--------+--------+--------------+
```

`codec` v0 values: `0x01` = H.264 Baseline.

### FrameHeader (type 0x01)

```
+--------+--------+--------------+--------+--------+
|frame_id| flags  | timestamp_us | n_slices | rsvd |
| :u32   | :u8    | :u64         | :u8    | :u8   |
+--------+--------+--------------+--------+--------+
```

Flags bits:
- `0x01` — IDR (keyframe)
- `0x02` — final frame before pause

### FrameSlice (type 0x02)

```
+--------+--------+--------+----------------+
|frame_id| slice  | total  | nal_unit_bytes |
| :u32   | _idx:u8| :u8    | <u32>          |
+--------+--------+--------+----------------+
```

`slice_idx` < `total_slices`. Slices may arrive out of order *within*
a stream is impossible (QUIC ordered), so client just appends as
received.

### FrameEnd (type 0x03)

```
+--------+
|frame_id|
| :u32   |
+--------+
```

Confirms all slices for `frame_id` were emitted. Client uses this to
release the decoded frame to the renderer.

---

## Input channel frames (kind 0x0004)

| Type | Name           | Description                        |
|------|----------------|-------------------------------------|
| 0x01 | KeyEvent       | Keyboard event                      |
| 0x02 | PointerMove    | Mouse / pointer move                |
| 0x03 | PointerButton  | Mouse / pointer button              |
| 0x04 | PointerWheel   | Scroll wheel                        |
| 0x05 | TouchEvent     | Touchscreen event                   |
| 0x06 | SyntheticBatch | Atomic batch (paste)                |
| 0x07 | TextInput      | Committed Unicode text (mobile/IME) |

### KeyEvent (type 0x01)

```
+----------+-----------+-----------+--------+
| keysym   | scancode  | modifiers | flags  |
| :u32     | :u32      | :u32      | :u8    |
+----------+-----------+-----------+--------+
```

`keysym` is an X11-keysym-compatible Unicode-mapped value. Where the
client cannot determine a keysym (mobile soft keyboards), it sets
`keysym = 0` and includes the typed character via the
SyntheticBatch path.

Modifier bits (mask):
- `0x0001` Shift
- `0x0002` Ctrl
- `0x0004` Alt
- `0x0008` Meta / Super / Cmd
- `0x0010` AltGr
- `0x0020` CapsLock
- `0x0040` NumLock
- `0x0080` ScrollLock

Flag bits:
- `0x01` down (1 = pressed, 0 = released)
- `0x02` repeat (auto-repeat synthetic)

### PointerMove (type 0x02)

```
+--------+--------+--------+--------+
| flags  | x      | y      | rsvd   |
| :u8    | :i32   | :i32   | :u8    |
+--------+--------+--------+--------+
```

Flags:
- `0x01` absolute (1 = (x,y) in pixels; 0 = relative delta)
- `0x02` from touch

### PointerButton (type 0x03)

```
+--------+--------+
| button | flags  |
| :u8    | :u8    |
+--------+--------+
```

Buttons: 1 = left, 2 = middle, 3 = right, 4 = back, 5 = forward.

Flag bit `0x01` = down.

### PointerWheel (type 0x04)

```
+--------+--------+--------+
| dx     | dy     | flags  |
| :i16   | :i16   | :u8    |
+--------+--------+--------+
```

Units: 1/120ths of a notch (matches Windows WHEEL_DELTA convention),
allowing high-resolution wheels and trackpads.

Flag bit `0x01` = high-precision (treat units as 1/10 px instead of
1/120 notch).

### TouchEvent (type 0x05)

```
+--------+--------+--------+--------+--------+
| touch  | phase  | x      | y      | press  |
| _id:u32| :u8    | :i32   | :i32   | :u16   |
+--------+--------+--------+--------+--------+
```

Phases: 0 = begin, 1 = move, 2 = end, 3 = cancel.

### SyntheticBatch (type 0x06)

```
+--------+
| n:u16  | followed by n inner frames, each with the outer envelope omitted
+--------+
```

The batch is applied atomically server-side: either all inner events
succeed, or none. Used for "paste as typing" fallback when the
clipboard is unavailable.

### TextInput (type 0x07)

```
+----------------+
| text : bytes<u32>  ; UTF-8, length-prefixed, max 64 KiB per message
+----------------+
```

The server treats `text` as committed text input and injects it as if
the user had typed it. Used by mobile virtual keyboards, emoji
pickers, voice-to-text, and the committed output of client-side IMEs.

The server MUST NOT distinguish between TextInput and an equivalent
sequence of KeyEvent / SyntheticBatch frames at the level of the
target application: both look like "the user typed these characters."

Modifier keys are NOT carried in TextInput. If the client needs to
send modified keystrokes (e.g., Ctrl-C), it MUST use KeyEvent.

---

## Cursor channel frames (kind 0x0003)

| Type | Name         | Description                       |
|------|--------------|------------------------------------|
| 0x01 | CursorMove   | Position update                    |
| 0x02 | CursorShape  | New cursor sprite                  |
| 0x03 | CursorHidden | Cursor not displayed               |

### CursorMove (type 0x01)

```
+--------+--------+
| x:i32  | y:i32  |
+--------+--------+
```

### CursorShape (type 0x02)

```
+--------+--------+--------+--------+--------+
| width  | height | hot_x  | hot_y  | format |
| :u16   | :u16   | :u16   | :u16   | :u8    |
+--------+--------+--------+--------+--------+
| pixels_length:u32 | pixels...                |
+--------+--------+--------+--------+--------+
```

`format` values:
- `0x01` 32-bit BGRA, premultiplied alpha.

(Other formats reserved for v1.)

---

## Clipboard channel frames (kind 0x0005)

| Type | Name             | Description                |
|------|------------------|----------------------------|
| 0x01 | OfferTypes       | Available MIME types       |
| 0x02 | RequestContent   | Ask for a specific type    |
| 0x03 | Content          | Response with bytes        |
| 0x04 | ContentChunk     | Chunked content (>1 MB)    |
| 0x05 | ContentEnd       | Final chunk marker         |

### OfferTypes (type 0x01)

```
CBOR array of tstr (MIME types)
```

### RequestContent (type 0x02)

```
CBOR map:
  1 (type)             : tstr
  2 (request_id)       : uint
```

### Content (type 0x03)

```
CBOR map:
  1 (request_id)       : uint
  2 (type)             : tstr
  3 (bytes)            : bstr   ; up to 1 MiB
  4 (more)             : bool   ; true → ContentChunks follow
```

### ContentChunk (type 0x04)

```
CBOR map:
  1 (request_id)       : uint
  2 (sequence)         : uint
  3 (bytes)            : bstr
```

### ContentEnd (type 0x05)

```
CBOR map:
  1 (request_id)       : uint
  2 (status)           : uint   ; 0 = OK
```

---

## File channel frames (kind 0x0006)

| Type | Name           | Description                  |
|------|----------------|-------------------------------|
| 0x01 | StartTransfer  | Begin a transfer              |
| 0x02 | Manifest       | Files + sizes + hashes        |
| 0x03 | Chunk          | One chunk of file data        |
| 0x04 | AckChunk       | Receipt / hash result         |
| 0x05 | EndTransfer    | Finalize                      |
| 0x06 | CancelTransfer | Abort                         |

Manifest layout (CBOR):

```
CBOR map:
  1 (transfer_id)      : uint
  2 (direction)        : tstr   ; "upload" | "download"
  3 (root)             : tstr   ; destination root path
  4 (entries)          : array of map:
      1 (path)         : tstr   ; relative path within transfer
      2 (size)         : uint   ; bytes
      3 (perms)        : uint   ; POSIX mode bits
      4 (mtime)        : uint   ; unix epoch seconds
      5 (chunks)       : array of bstr  ; SHA-256 per chunk
  5 (chunk_size)       : uint   ; bytes (default 262144)
  6 (root_hash)        : bstr   ; SHA-256 of concatenated per-chunk hashes
```

Chunk (binary):

```
+--------+--------+--------+----------------+
| transfer_id : u32                          |
+--------+--------+--------+----------------+
| file_index  : u32                          |
+--------+--------+--------+----------------+
| chunk_index : u32                          |
+--------+--------+--------+----------------+
| bytes : bytes<u32>                         |
+--------+--------+--------+----------------+
```

`bytes` length MUST equal the negotiated `chunk_size` except for the
last chunk of a file, which may be shorter.

AckChunk:

```
+--------+--------+--------+--------+
| transfer_id : u32                  |
+--------+--------+--------+--------+
| file_index  : u32                  |
+--------+--------+--------+--------+
| chunk_index : u32                  |
+--------+--------+--------+--------+
| status      : u8                   |
+--------+--------+--------+--------+
```

Status: `0` = OK, `1` = HASH_MISMATCH, `2` = OUT_OF_SPACE,
`3` = WRITE_ERROR.

---

## Audio channel frames (kind 0x0007)

| Type | Name        | Description           |
|------|-------------|------------------------|
| 0x01 | AudioParams | Codec parameters       |
| 0x02 | AudioFrame  | One Opus packet        |

### AudioParams (type 0x01)

```
+--------+--------+--------+
| codec  | sample | chan   |
| :u8    | _rate  | nels   |
|        | :u32   | :u8    |
+--------+--------+--------+
| bitrate_bps : u32         |
+--------+--------+--------+
```

`codec`: `0x01` = Opus.

### AudioFrame (type 0x02)

```
+----------------+----------------+
| seq : u32      | timestamp_us:u64 |
+----------------+----------------+
| opus_packet : bytes<u16>         |
+----------------+----------------+
```

---

## Constants summary

| Constant                       | Value         |
|--------------------------------|---------------|
| Protocol version (v0)          | `0x00`        |
| Max frame length               | 16 MiB        |
| Max Control message            | 1 MiB         |
| Default file chunk size        | 256 KiB       |
| Min file chunk size            | 4 KiB         |
| Max file chunk size            | 4 MiB         |
| Max clipboard payload          | 64 MiB        |
| Default resumption window      | 30 s          |
| ALPN identifier                | `"openrd/v0"` |
| Default port (QUIC/UDP)        | 443           |
| Default port (TLS/TCP fallback)| 443           |

## Resolved items

All wire-format open questions for v0 are resolved — see
[`decisions.md`](decisions.md):
- D6: TextInput message handles mobile/IME/emoji.
- D7: Hybrid CBOR profile (Preferred + Deterministic for signed).
- D20: IANA ALPN registration deferred to v1; v0 uses
  `"openrd/v0"` without IANA-registered status.

## Chat channel frames (kind 0x0009)

| Type | Name             | Description                          |
|------|------------------|---------------------------------------|
| 0x01 | ChatMessage      | UTF-8 text message                   |
| 0x02 | TypingIndicator  | start / stop                         |
| 0x03 | ChatAttachment   | Inline attachment up to 1 MiB        |

### ChatMessage (type 0x01)

```
CBOR map:
  1 (msg_id)     : uint    ; sender-allocated, unique within session
  2 (sender)     : tstr    ; authenticated identity from AuthResult.identity
  3 (body)       : tstr    ; UTF-8, up to 16 KiB
  4 (ts_ms)      : uint    ; sender's wall-clock ms since epoch
```

### TypingIndicator (type 0x02)

```
CBOR map:
  1 (sender)     : tstr
  2 (state)      : tstr    ; "start" | "stop"
```

### ChatAttachment (type 0x03)

```
CBOR map:
  1 (msg_id)     : uint
  2 (sender)     : tstr
  3 (mime)       : tstr    ; e.g. "image/png"
  4 (filename)   : tstr    ; optional, display only
  5 (bytes)      : bstr    ; up to 1 MiB
```

Attachments above 1 MiB MUST be transferred via the File channel and
referenced by file path or transfer ID in a regular ChatMessage.
