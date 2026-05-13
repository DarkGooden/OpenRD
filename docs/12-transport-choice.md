# 12 — Transport Choice

> Status: Draft v0.1
> Last updated: 2026-05-13

This document records *why* OpenRD v0 chooses its transport. The
choice has cascading consequences (channel model, security, web
client design) so the rationale is captured in one place.

**Decision: QUIC (RFC 9000) with mandatory TLS 1.3, plus a
TCP+TLS-over-WebSocket fallback for environments that block UDP.**

For browsers, that means **WebTransport** as the primary transport
and **WebSocket-over-HTTPS** as the fallback. For native clients,
**QUIC over UDP** primary and **TLS-over-TCP** fallback.

The rest of this document explains the alternatives we considered
and why we did not choose them.

## What we need from a transport

From [`requirements`](02-requirements.md):

1. **Multi-channel multiplexing with no head-of-line blocking
   between channels.** A file transfer must not stall input.
2. **Sub-30 ms LAN, sub-100 ms WAN latency budget.** The transport
   must be small.
3. **Strong, mandatory, modern encryption.** TLS 1.3 or equivalent.
4. **Connection migration.** UC-5 (mobile) and UC-6 (kiosk) require
   surviving an IP-address change.
5. **Session resumption** across short network outages.
6. **Browser support.** UC-2 demands a real web client.
7. **Works through corporate firewalls** that block UDP.
8. **Production-ready libraries** in at least Rust, C/C++, JavaScript
   (browser and Node), and ideally Go and Java.

## Alternatives considered

### A. TCP + TLS 1.3 (single stream)

The classic shape. RDP, VNC, SPICE all default to this.

- *Multiplexing:* No native multiplexing. You'd build your own
  message framing and multiplex inside one TCP stream. **Head-of-line
  blocking is unavoidable** — a TCP segment loss stalls every channel.
- *Latency:* Acceptable on LAN, suffers on lossy WAN.
- *Encryption:* TLS 1.3 is mature.
- *Migration:* TCP cannot migrate. A connection IP change kills it.
- *Browser:* TCP not directly available, but TLS-over-TCP via
  HTTP/2 or WebSocket is.
- *Libraries:* universal.

**Verdict: not enough.** Head-of-line and no migration are killers.

### B. TCP + TLS 1.3 + multiple parallel connections

What HTTP/1.1 did. Open *n* connections, distribute channels.

- *Multiplexing:* OK at coarse granularity but you pay for *n*
  handshakes, *n* congestion windows, *n* firewall holes.
- *Migration:* Still no.
- *Verdict:* Operationally ugly. NAT and firewall friction. Rejected.

### C. HTTP/2 + TLS 1.3

HTTP/2 streams give per-stream multiplexing.

- *Multiplexing:* Good — HTTP/2 streams are multiplexed within one
  TCP connection.
- *Head-of-line blocking:* **Still TCP underneath.** A packet loss
  stalls all HTTP/2 streams on that connection. This is HTTP/2's
  well-known weakness.
- *Migration:* No (TCP).
- *Browser:* Available via the standard fetch API + Server-Sent
  Events or streaming requests, but a full bidirectional protocol on
  top of HTTP/2 is awkward in the browser. WebSocket is what people
  actually use for that, which puts you back at one stream.
- *Verdict:* The TCP head-of-line problem is a deal-breaker for our
  channel model. Rejected.

### D. HTTP/3 (= QUIC under HTTP semantics)

QUIC but framed as HTTP/3 requests/streams.

- *Multiplexing:* Excellent — independent streams.
- *Migration:* Yes (QUIC feature).
- *Browser:* HTTP/3 fetch is available but the "request/response"
  semantics don't map well to long-lived bidirectional channels. You
  end up using fetch streams + Server-Sent Events, which is
  shoehorning.
- *Verdict:* The HTTP framing is unnecessary overhead for a
  non-HTTP protocol. **Pick QUIC directly.** Rejected as a primary
  but kept as a fallback option for environments that allow only
  HTTPS.

### E. QUIC (RFC 9000) directly

QUIC without HTTP on top.

- *Multiplexing:* Excellent — bidirectional and unidirectional streams,
  independent flow control, no head-of-line blocking *between*
  streams.
- *Encryption:* TLS 1.3 is mandatory in QUIC. We can't have a
  cleartext fallback even if we wanted one (NF-4.2 ✓).
- *Migration:* **Built in.** Connection ID is independent of
  IP+port. Mobile and kiosk cases (UC-5, UC-6) get migration with
  no extra protocol design.
- *Datagrams:* QUIC datagrams (RFC 9221) give us unreliable
  delivery when we want it (cursor, possibly display in v1).
- *Libraries:*
  - Rust: `quinn`, `s2n-quic`, `quiche`. All production-ready.
  - C/C++: `lsquic`, `msquic`, `picoquic`.
  - JS (browser): **WebTransport** is the QUIC-over-HTTP/3 API
    available in modern Chromium and is becoming widely supported.
    Firefox is implementing.
  - Node: WebTransport polyfill / lsquic bindings.
- *Browser fallback:* Where WebTransport is unavailable,
  WebSocket-over-HTTPS provides a TCP+TLS path. We accept the loss
  of features (no migration, head-of-line within the single stream)
  on the fallback path.
- *Firewalls:* UDP/443 is increasingly common (most HTTP/3 traffic
  uses it) but still blocked in some corporate environments. Hence
  the fallback.
- *Verdict:* **This is the choice.**

### F. WebRTC

WebRTC gives us SCTP-over-DTLS-over-UDP (for data channels), audio
and video tracks (with codec negotiation), and ICE for NAT traversal.

- *Multiplexing:* Yes (SCTP streams).
- *Encryption:* DTLS 1.2 is the floor; DTLS 1.3 is rolling out.
- *Migration:* Via ICE restart; complex.
- *Browser:* Native, mature.
- *Native:* `libwebrtc` is enormous. Independent implementations
  exist but the protocol stack is much heavier than QUIC.
- *Cons:*
  - Designed for browser-to-browser, not client-to-server. The
    signaling layer (ICE, STUN, TURN) is mandatory and adds
    operational complexity.
  - Codec selection and stream control are coupled with the
    media-track abstraction, which is great for AV conferencing
    but awkward for "send my arbitrary protocol messages."
  - DTLS 1.2 is older than TLS 1.3.
- *Verdict:* Powerful but the wrong fit. WebRTC excels at
  peer-to-peer media; OpenRD is client-to-server data + media.
  Rejected.

### G. ENet (Sunshine/Moonlight's choice)

A small reliable-UDP library, simple and effective.

- *Multiplexing:* Yes (channels).
- *Encryption:* None — you have to layer something on top.
- *Migration:* No.
- *Browser:* No.
- *Verdict:* Good for closed ecosystems; not viable for a public
  protocol that must run in a browser. Rejected.

## The chosen architecture

```
Native client                              Web client (modern)
+----------------------+                  +--------------------+
| OpenRD client core   |                  | OpenRD client core |
|         |            |                  |  (WASM build)      |
|       QUIC           |                  |         |          |
|     (UDP/443)        |                  |    WebTransport    |
+----------------------+                  +--------------------+
         |                                          |
         +----------------+      +------------------+
                          |      |
                          v      v
                     +-----------------+
                     | OpenRD server   |
                     |   QUIC endpoint |
                     +-----------------+
                          ^      ^
                          |      |
         +----------------+      +------------------+
         |                                          |
+----------------------+                  +--------------------+
| Native client (fallback)                 | Web client (fallback)
|     TLS 1.3 / TCP    |                  |   WebSocket / TLS  |
+----------------------+                  +--------------------+
```

**Native primary:** QUIC over UDP/443.
**Native fallback:** TLS 1.3 over TCP/443.
**Web primary:** WebTransport (which is QUIC).
**Web fallback:** WebSocket over HTTPS.

The fallback path uses the same wire format as the primary, with two
differences:

1. **Single QUIC stream → single TCP/WebSocket stream**, so channels
   are multiplexed inside it. Head-of-line blocking applies and is
   accepted as the cost of the fallback.
2. **No connection migration.** A network change reconnects.

## Port and ALPN

- **UDP/443** for QUIC.
- **TCP/443** for the TLS fallback.
- **ALPN identifier**: `"openrd/v0"` (registered via the spec; we will
  request a code point from IANA's TLS ALPN registry when leaving
  pre-alpha).

Co-locating on 443 maximizes reachability through corporate
firewalls.

## Stream-to-channel mapping (preview)

Detailed in [`20-wire-format-v0.md`](20-wire-format-v0.md), but
in summary:

- Control channel → QUIC bidirectional stream #0.
- Display channel → QUIC unidirectional stream (server-initiated).
- Cursor channel → QUIC unidirectional stream (server-initiated).
- Input channel → QUIC unidirectional stream (client-initiated).
- Clipboard channel → QUIC bidirectional stream.
- File channel → QUIC bidirectional stream, one per transfer.
- Audio channel → QUIC unidirectional stream + optional datagrams.

## Open questions

- Should we mandate QUIC datagrams for the Audio channel in v0, or
  only "MAY"? Current lean: SHOULD; clients without datagram support
  fall back to the stream.
- Should we require QUIC implementations to support 0-RTT for
  resumption? Current lean: SHOULD; security caveats around 0-RTT
  replay must be handled at the auth layer.
- HTTP/3 ALPN tunneling: do we provide an explicit HTTP/3 framing
  for environments that strictly require HTTP semantics on UDP/443?
  Current lean: no in v0; revisit if real-world deployments hit
  firewalls that allow HTTP/3 but not raw QUIC.
