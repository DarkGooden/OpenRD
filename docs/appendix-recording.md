# Appendix — Session Recording (Informative)

> Status: Informative (non-normative)
> Last updated: 2026-05-13

This appendix is **non-normative**. Implementations may record
sessions in any format they choose. This appendix describes a
*recommended* baseline format so that recordings made by one
implementation can be replayed by tools written by another.

## Baseline format

A session recording is a single file with two parts:

1. **Manifest** — a CBOR-encoded header describing the session.
2. **Frame log** — a sequence of timestamped wire frames captured at
   the server.

### File layout

```
+----------+--------+--------+--------------+----------+
| magic    | ver:u8 | rsvd:u8| manifest_len | manifest |
| :u32 (*) |        |        | :u32         | :bytes   |
+----------+--------+--------+--------------+----------+
| frame log: a sequence of frame records, each:        |
|   timestamp_ns : u64   (monotonic, captured at server)|
|   channel_id   : u32                                  |
|   direction    : u8    (0 = c->s, 1 = s->c)           |
|   frame_len    : u32                                  |
|   frame_bytes  : bytes  (the full OpenRD wire frame   |
|                          including outer envelope)    |
+------------------------------------------------------+
```

`magic` = `0x4F525244` ("ORRD" little-endian, "Open RD" Recording).
`ver` = `0x00` for this baseline format.

The frame log MUST be ordered by `timestamp_ns`.

### Manifest (CBOR)

```
CBOR map:
  1 (session_id)            : bstr (16 bytes)
  2 (started_at_unix)       : uint
  3 (ended_at_unix)         : uint    ; 0 if unknown / still in progress
  4 (server_identity)       : tstr
  5 (client_identity)       : tstr
  6 (negotiated_profile)    : map     ; see capability negotiation
  7 (recorder_version)      : tstr    ; software version of the recorder
  8 (notes)                 : tstr    ; optional free-form
```

## Replay semantics

A replay tool reads the manifest, then walks the frame log emitting
each frame to a virtual session at its recorded timestamp. The
recorded session is deterministic at the protocol level; rendered
output may differ if the replay client uses different decoders or
fonts than the original.

A replay tool MUST NOT inject input events into a live system as part
of a replay. Replays are for analysis only.

## What this baseline does NOT cover

- **Encryption at rest** — operators that need this should wrap the
  format in an encrypted container of their choice.
- **Redaction** — removing or masking sensitive content (passwords
  typed, secrets pasted) is out of scope for the baseline.
- **Compression** — apply standard compression (zstd, xz) to the
  outer file if size matters.
- **Indexing** — for long sessions, a separate index file with
  `(timestamp_ns, file_offset)` pairs at one-second intervals can make
  seek operations fast. Format and naming convention are
  implementation-defined.

## Why an appendix and not part of the normative spec

Per [`decisions.md`](decisions.md) D5: recording is an operator
concern, not a protocol concern. The protocol's wire format is
already deterministic and replayable; this appendix simply documents
the minimum container for cross-implementation interop.
