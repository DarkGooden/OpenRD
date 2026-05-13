# 21 — State Machines

> Status: Draft v0.1
> Last updated: 2026-05-13

This document specifies the connection lifecycle and the per-channel
lifecycle as explicit state machines. Implementations MUST behave as
described; "MAY" is reserved for transitions explicitly marked
optional.

## Notation

- States in `BOXES`.
- Events / inputs in *italics*.
- Actions in `monospace`.
- Default unspecified transition: drop the event, log a warning.

---

## Connection / session state machine

This is the master state machine for an OpenRD session, owned by
both the client and the server (each side runs an instance and they
must stay in agreement via the Control channel).

```
                     +-----------------+
   +---------------> |  DISCONNECTED   |
   |                 +--------+--------+
   |                          |
   |              client initiates QUIC handshake
   |                          |
   |                          v
   |                 +-----------------+
   |                 |  HANDSHAKING    |
   |                 +--------+--------+
   |                          |
   |        QUIC handshake success
   |                          |
   |                          v
   |                 +-----------------+
   |                 |  HELLO_EXCHANGE |  (ClientHello / ServerHello)
   |                 +--------+--------+
   |                          |
   |           version OK     |   version mismatch
   |        +-----------------+--------------+
   |        |                                |
   |        v                                v
   |  +-----------+                  +--------------------+
   |  | AUTHING   |                  | terminate w/ Error  |
   |  +-----+-----+                  +---------+----------+
   |        |                                  |
   |        |  AuthRequest / AuthResult        |
   |        |                                  |
   |        | success                  failure |
   |        +------------+----------------------+
   |                     |
   |                     v
   |              +-------------+
   |              |  READY      | <----+
   |              +------+------+      |
   |                     |             |
   |     network blip / migration      |
   |                     |             |
   |                     v             |
   |              +-------------+      |
   |              |  DEGRADED   | -----+ (transport recovers)
   |              +------+------+
   |                     |
   |     timeout > resumption window
   |                     |
   |                     v
   |              +-------------+
   |              | SUSPENDED   |
   |              +------+------+
   |                     |
   |    SessionResume succeeds  +--->  READY
   |    SessionResume fails / window expired
   |                     |
   |                     v
   |              +-------------+
   +--------------+   CLOSED    |
                  +-------------+
```

### State definitions

- **DISCONNECTED.** No QUIC connection. No session state.
- **HANDSHAKING.** QUIC + TLS 1.3 in progress. No application bytes
  yet.
- **HELLO_EXCHANGE.** QUIC ready; Control stream open; capability
  hello frames being exchanged. No other channel may be opened.
- **AUTHING.** Auth round-trip in progress. The server may emit
  zero or more AuthChallenges (for PIN flow) before the final
  AuthResult.
- **READY.** Steady state. All channels are openable. Display can be
  streaming.
- **DEGRADED.** QUIC reports loss / migration in progress / extended
  silence. Application-layer behavior MUST be: pause non-critical
  channels (Audio), keep Control alive with Ping, defer file work,
  hold Input events for replay if RTT allows.
- **SUSPENDED.** Transport is gone; the server is holding the
  session for up to the resumption window. A client may attempt a
  fresh QUIC connection + SessionResume during this period.
- **CLOSED.** Terminal. All resources released. To use the session
  again, start over from DISCONNECTED.

### Transitions

| From            | Event                                    | To               | Action                                                  |
|------------------|-------------------------------------------|------------------|----------------------------------------------------------|
| DISCONNECTED     | client.connect()                          | HANDSHAKING      | start QUIC handshake                                     |
| HANDSHAKING      | QUIC handshake succeeds                   | HELLO_EXCHANGE   | open Control stream                                      |
| HANDSHAKING      | QUIC handshake fails                      | CLOSED           | report transport error                                   |
| HELLO_EXCHANGE   | matching versions / capabilities          | AUTHING          | next: AuthRequest                                        |
| HELLO_EXCHANGE   | version mismatch                          | CLOSED           | send Error(UNSUPPORTED_VERSION), close                  |
| AUTHING          | AuthResult.status == 0                    | READY            | allow non-Control channel opens                          |
| AUTHING          | AuthResult.status != 0                    | CLOSED           | report AUTH_FAILED                                       |
| READY            | QUIC reports path degradation             | DEGRADED         | start Ping cadence, pause Audio                          |
| READY            | client closes / server kicks              | CLOSED           | tear down                                                 |
| DEGRADED         | QUIC reports path recovered                | READY            | resume Audio                                              |
| DEGRADED         | total silence > T_quic_idle                | SUSPENDED        | server retains session state                              |
| SUSPENDED        | client opens new QUIC + SessionResume OK  | READY            | reattach existing channels                                |
| SUSPENDED        | resumption window elapsed                 | CLOSED           | tear down session                                         |
| any              | fatal Error received                      | CLOSED           | tear down                                                 |

### Timers

- `T_quic_idle` — when QUIC's own idle timer fires (default 30 s for
  QUIC), or no application-level message for `T_idle_app` (default
  10 s in READY, 2 s in DEGRADED).
- `T_resumption_window` — default 30 s, configurable via capabilities
  during HELLO_EXCHANGE.
- `T_ping` — default 5 s in READY, 1 s in DEGRADED.

### Authentication round-trip detail

```
client                                  server
  |                                       |
  |---- ClientHello ---------------------->|
  |<--- ServerHello ----------------------|  state: HELLO_EXCHANGE
  |                                       |
  |  (if resuming)                        |
  |---- SessionResume -------------------->|
  |<--- SessionResumed (or rejection) ----|
  |                                       |
  |  (if not resuming)                    |
  |---- AuthRequest --------------------->|
  |                                       |
  |  (loop, optional)                     |
  |<--- AuthChallenge --------------------|
  |---- AuthRequest (continued) --------->|
  |                                       |
  |<--- AuthResult ----------------------|  state: READY (if OK)
  |                                       |
```

---

## Channel state machine

Each channel has its own state machine, independent of all other
channels. The Control channel is special: it opens implicitly when
the session enters HELLO_EXCHANGE, and exists for the life of the
session.

```
              +-----------------+
              |   IDLE          |
              +--------+--------+
                       |
        OpenChannel sent / received
                       |
                       v
              +-----------------+
              |   OPENING       |
              +--------+--------+
                       |
              OpenChannelAck (OK)
                       |
                       v
              +-----------------+
              |   OPEN          |
              +--------+--------+
                       |
                CloseChannel sent / received
                       |
                       v
              +-----------------+
              |   CLOSING       |
              +--------+--------+
                       |
              CloseChannelAck or QUIC stream FIN
                       |
                       v
              +-----------------+
              |   CLOSED        |  (terminal)
              +-----------------+
```

### Channel events

| From    | Event                          | To       | Action                                          |
|---------|--------------------------------|----------|--------------------------------------------------|
| IDLE    | local opens                    | OPENING  | send OpenChannel                                 |
| IDLE    | OpenChannel received           | OPENING  | check permission; send OpenChannelAck or refuse  |
| OPENING | OpenChannelAck(OK)             | OPEN     | begin data flow                                  |
| OPENING | OpenChannelAck(error)          | CLOSED   | release stream ID                                |
| OPENING | timeout                        | CLOSED   | send CloseChannel(timeout), release             |
| OPEN    | data                            | OPEN     | process per channel kind                         |
| OPEN    | CloseChannel sent / received   | CLOSING  | flush remaining data, send CloseChannelAck      |
| OPEN    | QUIC stream reset by peer       | CLOSED   | log error                                        |
| OPEN    | session enters DEGRADED         | OPEN     | reduce activity per channel-kind policy          |
| OPEN    | session enters SUSPENDED        | suspended| retain state; pause writes                       |
| CLOSING | CloseChannelAck                 | CLOSED   | release stream ID                                |

### Channel-kind-specific behavior in DEGRADED

| Channel kind | Behavior in DEGRADED                                |
|--------------|------------------------------------------------------|
| Control      | Keep Ping cadence; do not open new channels         |
| Display      | Stop emitting new frames; keep last frame on screen |
| Cursor       | Pause                                                |
| Input        | Hold last input; suppress repeats                   |
| Clipboard    | Pause                                                |
| File         | Pause (do not emit chunks); resume on READY         |
| Audio        | Pause; do not buffer                                 |
| Stats        | Pause                                                |

---

## File-transfer state machine (within a File channel)

A single File channel hosts one transfer, identified by its
transfer_id. The channel state machine above applies, plus this
transfer-level state machine:

```
              +-----------------+
              |   NEGOTIATING   |  (StartTransfer + Manifest exchange)
              +--------+--------+
                       |
                  manifest OK
                       |
                       v
              +-----------------+
              |   TRANSFERRING  |
              +--------+--------+
                       |
              all chunks acked
                       |
                       v
              +-----------------+
              |   VERIFYING     |  (optional, hash check)
              +--------+--------+
                       |
                       v
              +-----------------+
              |   DONE          |
              +-----------------+
```

If a chunk fails verification, the receiver responds with
AckChunk(status = HASH_MISMATCH) and the sender retransmits that
chunk. If a chunk fails three times, the sender emits
CancelTransfer.

Resumption: when the session moves from SUSPENDED → READY, an
in-flight transfer may resume. The receiver advertises the last
contiguous acked chunk; the sender skips ahead.

---

## Consent flow

Some actions require explicit user consent (T-13 mitigation, UC-4):
permission elevation, accepting an inbound file with overwrite, etc.

```
server                                 client (user)
  |                                       |
  |---- ConsentRequest ------------------>|
  |     (action, details, timeout_ms)     |
  |                                       |
  |                                       |   [UI prompt shown]
  |                                       |
  |<--- ConsentResponse ------------------|
  |     (granted: bool)                   |
  |                                       |
  |  if granted: server proceeds          |
  |  if denied: server emits Error        |
  |  if timeout: server emits Error       |
```

The server MUST wait for ConsentResponse or for the timeout; it MUST
NOT perform the gated action without an explicit `granted = true`.

---

## Pseudocode for the server's READY loop

This is a non-normative reference sketch.

```
loop {
    select {
        msg = control_stream.recv() => handle_control(msg)
        ev  = capture.next_frame()   => display_stream.send(ev)
        cev = cursor.next_event()    => cursor_stream.send(cev)
        iev = input_stream.recv()    => apply_input(iev)
        cb  = clipboard.next_op()    => handle_clipboard(cb)
        f   = file_channels.next()    => handle_file(f)
        au  = audio.next_packet()    => audio_stream.send(au)
        _   = ping_timer.tick()      => control_stream.send(Ping {...})
        _   = idle_timer.tick()      => if no activity, transition DEGRADED
    }
}
```

The READY loop must be careful to **never block on any one channel**.
Channel handlers run in their own tasks; the main loop schedules I/O.

---

## Open items

- Should resumption be allowed across QUIC migrations within the same
  connection, or only via fresh connections? Currently: QUIC migration
  is transport-level and never requires SessionResume. SessionResume
  is for crossing the QUIC connection boundary entirely.
- Should the consent flow include a "remember for this session"
  option in the wire format, or is that a client-side UX detail?
  Lean: client-side UX.
