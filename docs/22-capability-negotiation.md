# 22 — Capability Negotiation

> Status: Draft v0.1
> Last updated: 2026-05-13

Capability negotiation is the step that decides *what features this
particular session will use* from the menu of options the protocol
defines. It happens once, immediately after the QUIC handshake, on
the Control channel.

## Goals of negotiation

1. **Forward compatibility.** A v0 client and a v1 server (or vice
   versa) MUST be able to establish a working v0 session.
2. **Graceful degradation.** Optional features may be requested by
   either side and silently declined by the other; the session
   continues without them.
3. **Explicit feature gates.** Any feature whose absence changes
   observable behavior MUST be negotiated, not assumed.
4. **No silent downgrade.** Security-sensitive features MUST NOT
   be silently dropped; if the server requires interactive auth and
   the client offers only view-only, the server MUST reject.
5. **Atomic decision.** The negotiation is one round trip; there is
   no haggling.

## When it happens

In the connection state machine:

```
HANDSHAKING --> HELLO_EXCHANGE --> AUTHING --> READY
                ^             ^
                |             |
                +-- capability negotiation --+
```

Capability descriptors are carried in `ClientHello` and `ServerHello`
on the Control channel. They are exchanged **before** authentication
so that the auth method itself is negotiable.

The server's `ServerHello` is the **final word**. Whatever the server
declares in `ServerHello.capabilities` is what this session will use.

## Capability schema

Capabilities are a CBOR map keyed by short integers. v0 keys:

| Key  | Capability                | Type                | v0 default              |
|------|----------------------------|---------------------|--------------------------|
| 1    | protocol_versions          | array of uint       | `[0]`                    |
| 2    | profile                    | tstr                | `"openrd-v0-base"`       |
| 3    | auth_methods               | array of tstr       | `["token", "pin", "mtls", "oidc"]` (intersection) |
| 4    | display_codecs             | array of tstr       | `["h264-baseline"]`      |
| 5    | display_max_resolution     | [width, height]     | `[1920, 1080]`           |
| 6    | display_max_fps            | uint                | `60`                     |
| 7    | audio_codecs               | array of tstr       | `["opus"]`               |
| 8    | clipboard_types            | array of tstr       | `["text/plain;charset=utf-8", "image/png"]` |
| 9    | file_max_concurrent        | uint                | `4`                      |
| 10   | file_max_size_per_transfer | uint (bytes)        | `17179869184` (16 GiB)   |
| 11   | file_chunk_size_range      | [min, max] (bytes)  | `[4096, 4194304]`        |
| 12   | clipboard_max_size         | uint (bytes)        | `67108864` (64 MiB)      |
| 13   | resumption_window_seconds  | uint                | `30`                     |
| 14   | transport_features         | array of tstr       | `["quic-datagrams"]` if supported, else `[]` |
| 15   | extensions                 | map of tstr → any   | `{}`                      |

### Detailed semantics

#### `1 protocol_versions`

Each side advertises an ordered list of supported versions. The
chosen version is `max(client ∩ server)`. v0 supports only `[0]`.
No overlap → `Error(UNSUPPORTED_VERSION)`, fatal.

#### `2 profile`

A named conformance profile. v0 defines exactly one:
`"openrd-v0-base"`. A profile bundles a set of mandatory
capabilities; declaring conformance to a profile is equivalent to
declaring its mandatory set.

Future profiles (e.g., `"openrd-v1-mobile"`) would define different
mandatory sets.

#### `3 auth_methods`

Both sides list the auth methods they understand. The intersection
is the negotiation result. If empty → `Error(AUTH_FAILED)` with the
text "no shared auth method", fatal.

The server's preference order MUST be honored; the client's order
is informational.

#### `4 display_codecs`

Both sides list codecs they understand. v0 mandates support for
`"h264-baseline"` on both sides. Future codecs go here. The server
picks one from the intersection and uses it for Display channel
frames.

#### `5 display_max_resolution`, `6 display_max_fps`

Hints. The Display channel will operate at min(client cap,
server cap). The client MAY request a lower resolution at any time
via a `SessionEvent("resolution_change")`.

#### `7 audio_codecs`

v0 mandates `"opus"`. Future codecs go here.

#### `8 clipboard_types`

MIME types each side can produce or consume. The server MUST accept
`text/plain;charset=utf-8` and `image/png` at minimum.

#### `9 file_max_concurrent`

Maximum number of File channels that may be open simultaneously.
Effective limit is the min of both sides' values.

#### `10 file_max_size_per_transfer`

Largest single transfer the side will accept as receiver.

#### `11 file_chunk_size_range`

The receiver's tolerable chunk size range. The sender picks a
specific chunk size during `StartTransfer` within
`[max(min_client, min_server), min(max_client, max_server)]`.

#### `12 clipboard_max_size`

Largest clipboard content payload the side will accept.

#### `13 resumption_window_seconds`

How long the side is willing to hold a suspended session. Effective
value is `min(client_window, server_window)`. The server MUST honor
its declared window; the client MAY abandon resumption earlier.

#### `14 transport_features`

Optional transport-level capabilities understood by the application
layer:

- `"quic-datagrams"` — the side can handle QUIC datagrams (RFC 9221)
  for use on Cursor and Audio channels in v1. v0 currently uses
  streams for everything, but advertising datagrams sets up forward
  compatibility.
- `"tcp-fallback"` — the side will accept a TCP+TLS fallback (server
  side).

#### `15 extensions`

A map for vendor extensions. Each key is a reverse-DNS-style string
(e.g., `"com.acme.frobnicate"`); the value is opaque to the spec.
Both sides MUST ignore extensions they do not understand.

Extensions MUST NOT change protocol-mandated behavior. They may add
optional channels (in the 0x8000–0xFFFF range) or optional Control
messages (in a vendor-specific Control message type range, TBD).

---

## Negotiation outcome

After ServerHello, both sides build a **NegotiatedProfile** struct
locally:

```
NegotiatedProfile {
    version:               0,
    profile:               "openrd-v0-base",
    auth_method:           <chosen, e.g. "pin">,
    display_codec:         "h264-baseline",
    display_resolution:    [W, H],
    display_max_fps:       <min of caps>,
    audio_codec:           "opus",
    clipboard_types:       <intersection>,
    file_max_concurrent:   <min>,
    file_max_size:         <min>,
    file_chunk_size_range: <intersection>,
    clipboard_max_size:    <min>,
    resumption_window:     <min>,
    transport_features:    <intersection>,
    extensions:            <enabled by mutual presence>,
}
```

This struct is the authoritative reference for the rest of the
session. Components MUST query it rather than re-deriving from
hellos.

---

## Versioning and forward compatibility

v0 is intentionally narrow: one codec, one auth profile, one
channel set. v1+ will add capabilities. Two principles for adding
capabilities later:

1. **Adding a capability key.** Older peers do not advertise it
   and do not understand it. The protocol behavior MUST default to
   "off" when either side is silent on the key.

2. **Adding values to an existing list (codec, auth method, etc.).**
   Older peers do not list the new value; intersection works
   naturally.

Removing capabilities or changing semantics of existing keys is a
breaking change and requires a new `protocol_versions` value.

### Negotiating against a future version

A v1 server talking to a v0 client:
- `protocol_versions`: client `[0]`, server `[0, 1]`. Chosen: `0`.
- The server MUST then behave as a v0 server for the duration of the
  session.

A v0 server talking to a v1 client:
- Same result: chosen version is `0`. Client downgrades.

This means a v1 server can be deployed safely in front of a fleet
of v0 clients.

---

## Worked example: minimal handshake

```
client → server: ClientHello {
    protocol_version: 0,
    client_name: "openrd-web/0.1.0",
    capabilities: {
        1: [0],
        2: "openrd-v0-base",
        3: ["pin", "token"],
        4: ["h264-baseline"],
        5: [1920, 1080],
        6: 30,
        7: ["opus"],
        8: ["text/plain;charset=utf-8", "image/png"],
        9: 2,
        10: 1073741824,           // 1 GiB
        11: [16384, 1048576],
        12: 4194304,
        13: 15,
        14: [],
        15: {}
    }
}

server → client: ServerHello {
    protocol_version: 0,
    server_name: "openrd-server/0.1.0",
    capabilities: {
        1: [0],
        2: "openrd-v0-base",
        3: ["pin"],              // server only enabled PIN
        4: ["h264-baseline"],
        5: [1920, 1080],
        6: 30,
        7: ["opus"],
        8: ["text/plain;charset=utf-8", "image/png", "text/html"],
        9: 4,
        10: 17179869184,
        11: [4096, 4194304],
        12: 67108864,
        13: 30,
        14: ["quic-datagrams"],
        15: {}
    },
    session_id: <16 bytes>,
    server_time: 1731494400
}

// Negotiated:
// auth_method = "pin"
// display = h264-baseline @ 1920x1080 / 30 fps
// clipboard types = {text/plain, image/png}
// file = up to 1 GiB, 2 concurrent, chunks 16384-1048576
// clipboard max = 4 MiB
// resumption window = 15 s
// transport features = {}
// extensions = {}
```

---

## Failure cases

| Failure                                       | Outcome                                                         |
|-----------------------------------------------|------------------------------------------------------------------|
| No overlapping `protocol_versions`             | `Error(UNSUPPORTED_VERSION)`, fatal                              |
| No overlapping `auth_methods`                  | `Error(AUTH_FAILED, "no shared method")`, fatal                  |
| No overlapping `display_codecs`                | `Error(NOT_IMPLEMENTED, "no shared display codec")`, fatal       |
| No overlapping `audio_codecs`                  | Session continues; Audio channel cannot be opened                |
| No overlapping `clipboard_types`                | Session continues; Clipboard cannot transfer (NegotiatedProfile records empty types) |
| `clipboard_max_size` too small for desired op | Local error; user is informed; falls back to File channel       |
| Server profile != client profile              | Session continues if both profiles share the v0 base; otherwise `Error(NOT_IMPLEMENTED)` |

---

## Open items

- Should the profile name be replaced with a version+optional-extension
  bitmask? Current lean: keep the string for human readability.
- A registry for `transport_features` strings: who maintains it?
  v0 ships with two; the rest are TBD.
- Whether the negotiation should be re-runnable mid-session (for
  example, after permission elevation). Current lean: no — the
  NegotiatedProfile is fixed at session start. Permission level
  is tracked separately and CAN change at runtime via consent.
