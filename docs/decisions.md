# Decisions Log

> Status: Final for v0
> Last updated: 2026-05-13

This document records the 21 design decisions resolved during initial
design review. Each entry is short on purpose: the docs themselves are
the spec; this log captures *what we decided and why* so future
maintainers can revisit a decision with the original reasoning in
hand.

If a decision in this log conflicts with text elsewhere in the docs,
this log wins until the conflicting doc is updated.

---

## D1 — Multi-monitor support

**Decision:** Defer to v1.

**Rationale:** v0 is the learning release. Adding multi-monitor roughly
doubles the design surface (per-monitor capability negotiation, cursor
coordinate spaces, encode budget juggling, capture path). The v0 → v1
addition is purely additive (open more Display channels), so no
forward compatibility cost.

Affects: `00-goals-and-non-goals.md`, `11-channel-model.md`.

---

## D2 — Bitmap / glyph cache for text

**Decision:** Defer to v1. Reserve Control frame types for future cache
extension.

**Rationale:** Real implementation surface (cache eviction, overlay
sync to Display frame IDs, stable-region detection). v0 ships with
H.264-only; if real-world text quality is a problem, v1 adds the cache
with empirical evidence behind the design.

Affects: `11-channel-model.md`, `20-wire-format-v0.md`.

---

## D3 — Smart-card and USB redirection

**Decision:** Both v2.

**Rationale:** Each is a 50–100 page sub-spec in its own right
(APDU-over-channel for smart card; URB-over-channel for USB).
Audience is real but small. v1 has plenty (multi-monitor, cache, mic,
NAT). Defer.

Affects: `00-goals-and-non-goals.md`.

---

## D4 — Chat channel in v0

**Decision:** Dedicated Chat channel in v0 (kind `0x0009`).

**Rationale:** Support scenarios (UC-2) need real-time chat alongside
the screen share, and embedding scenarios (UC-4) benefit from
single-connection-does-everything. Standardizing means implementations
interop on chat without each rolling its own vendor extension.

Scope of v0 Chat channel:
- Plain-text messages (UTF-8).
- Typing indicators (start, stop).
- Small inline attachments up to 1 MiB (above that, use File channel).
- No read receipts, no edit/delete, no threading in v0 — keep minimal.

Affects: `11-channel-model.md`, `20-wire-format-v0.md`,
`22-capability-negotiation.md`.

---

## D5 — Session recording

**Decision:** Informative appendix in v0 (non-normative).

**Rationale:** Almost-free interop win. The wire format is already
replayable; a one-paragraph spec ("timestamped frames + manifest
header") gives recording and playback tools cross-implementation
compatibility without forcing every server to implement it.

Affects: new appendix in `docs/`.

---

## D6 — Mobile keyboards / IME / emoji

**Decision:** Add a `TextInput { text: utf-8 }` message to the Input
channel in v0. Full IME composition support deferred to v1.

**Rationale:** Mobile virtual keyboards, emoji pickers, and committed
IME text all produce *finished* Unicode. A TextInput message handles
all three cleanly. CJK users compose on their local IME (client-side)
and send committed text. Server-side composition (with candidate
windows visible on the remote desktop) is rare and deferred.

Affects: `20-wire-format-v0.md` (Input channel section).

---

## D7 — CBOR encoding profile

**Decision:** Hybrid — Preferred Serialization for normal Control
messages, Deterministic Encoding for signed structures (resumption
tokens, invitation tokens, anything that gets re-hashed).

**Rationale:** Most CBOR libraries default to Preferred; pushing
Deterministic everywhere has parser cost for no benefit on most
traffic. Signed structures need canonical bytes for receiver-side
verification. One spec paragraph covers both.

Affects: `20-wire-format-v0.md`, `22-capability-negotiation.md`.

---

## D8 — Multi-monitor channel shape (v1 intent)

**Decision (non-binding, v1 intent):** N Display channels, one per
monitor.

**Rationale:** Matches the existing single-channel model. Each
monitor at 1080p30 CPU-encode is independent. Wire format is already
ready (channel instance IDs).

Affects: `11-channel-model.md` (v1 notes).

---

## D9 — Stats channel scope

**Decision:** Protocol metrics only — RTT, loss, retransmits, encode
time, decode time, queue depths, per-channel throughput. No host
process metrics.

**Rationale:** Host metrics belong in the operator's monitoring
stack (Prometheus / Datadog / OpenTelemetry). Putting them in the
protocol creates OS-coupling and scope creep.

Affects: `11-channel-model.md`.

---

## D10 — QUIC datagrams for Audio

**Decision:** SHOULD support. Negotiated via the
`"quic-datagrams"` value in `transport_features`. Falls back to
unidirectional stream when either peer can't do datagrams.

**Rationale:** Audio quality under loss is one of the most
user-noticeable artifacts. SHOULD pushes implementations to support
datagrams without excluding QUIC libraries that haven't added them yet.

Affects: `12-transport-choice.md`, `22-capability-negotiation.md`.

---

## D11 — 0-RTT for session resumption

**Decision:** SHOULD allow 0-RTT, but only for the `SessionResume`
frame.

**Conditions:**

1. Resumption tokens are 128+ bits of random.
2. Tokens are single-use; server issues a fresh token on every
   successful resume; old one is immediately invalidated.
3. Replay of a used token returns `RESUMPTION_REJECTED`.
4. 0-RTT carries *only* SessionResume — never any other frame.

**Rationale:** Saves ~50–150 ms per reconnect for mobile (UC-5) and
kiosk (UC-6). Same safety model as TLS 1.3 early-data and HTTP/3.

Affects: `12-transport-choice.md`, `21-state-machines.md`.

---

## D12 — Resumption vs QUIC migration

**Decision:** Strict separation. QUIC migration is transport-only and
transparent. SessionResume is only for *new* QUIC connections (when
the previous one died entirely).

**Rationale:** One clear mental model. If QUIC migration fails, the
connection dies; client opens a new QUIC connection and uses
SessionResume. No mid-connection resume on the same QUIC connection.

Affects: `21-state-machines.md`.

---

## D13 — Consent "remember for session"

**Decision:** Client-side UX only. Server treats every
`ConsentRequest` as independent.

**Rationale:** Client UI can suppress redundant prompts locally
based on its own policy. The protocol shouldn't model UX patterns.

Affects: `21-state-machines.md` (consent flow notes).

---

## D14 — Profile naming

**Decision:** String name. v0 profile: `"openrd-v0-base"`.

**Rationale:** Human-readable, debuggable, matches ALPN and HTTP
version precedent. The structured details already live in the
capabilities map; a parallel bitmask would invite "the bitmask and
the map disagree" bugs.

Affects: `22-capability-negotiation.md`.

---

## D15 — Re-runnable negotiation mid-session

**Decision:** No. The `NegotiatedProfile` is fixed at session start
and immutable for the session's lifetime.

**Rationale:** Renegotiation invites downgrade attacks (T-15) and
complex state machines. Permission level changes are handled by a
*separate* consent flow that does not re-run capability negotiation.

Affects: `22-capability-negotiation.md`.

---

## D16 — Additional Perfect Forward Secrecy beyond TLS 1.3

**Decision:** No additional requirement. TLS 1.3's mandatory ephemeral
key exchange is sufficient.

**Rationale:** TLS 1.3 already mandates PFS. Adding layer-7 PFS on
top is redundant and would complicate the protocol for no security
gain.

Affects: `03-threat-model.md`.

---

## D17 — Post-quantum / hybrid KEM

**Decision:** Defer to v1. Track the TLS 1.3 hybrid post-quantum
rollout (X25519+ML-KEM).

**Rationale:** TLS 1.3 hybrid suites are stabilizing in the IETF and
becoming available in production TLS libraries. v0 inherits whatever
the underlying TLS 1.3 stack supports. v1 will make hybrid PQ
mandatory once widely available.

Affects: `03-threat-model.md`.

---

## D18 — Vendor extension registry

**Decision:** Free space in v0. Channel kinds `0x8000–0xFFFF` and
extension keys in capability map are unmanaged. No central registry.

**Rationale:** v0 deployment scale doesn't justify the registry
overhead. Revisit at v1 if collisions become a problem in practice.

Affects: `11-channel-model.md`, `22-capability-negotiation.md`.

---

## D19 — `transport_features` value registry

**Decision:** Spec-author-maintained list. v0 ships with two values:
`"quic-datagrams"` and `"tcp-fallback"`. New values require a spec
update (i.e., a numbered version of this doc).

**Rationale:** Small, slow-moving list. Doesn't justify external
registry infrastructure.

Affects: `22-capability-negotiation.md`.

---

## D20 — IANA ALPN registration

**Decision:** Defer formal IANA registration to v1. v0 uses the
literal ALPN identifier `"openrd/v0"` without claiming IANA-registered
status.

**Rationale:** ALPN registration is a one-way door (codepoints are
permanent). We register once v1 stabilizes, so we know the identifier
isn't going to change.

Affects: `12-transport-choice.md`, `20-wire-format-v0.md`.

---

## D21 — HTTP/3 ALPN tunneling

**Decision:** No HTTP/3 framing in v0. v0 uses raw QUIC (ALPN
`openrd/v0`) plus the TCP+TLS-over-WebSocket fallback.

**Rationale:** Adding HTTP/3 framing is unnecessary indirection. We
revisit only if real-world deployments hit firewalls that
specifically allow HTTP/3 (UDP/443 with `h3` ALPN) but block raw QUIC
(UDP/443 with other ALPNs) — a niche today.

Affects: `12-transport-choice.md`.

---

## Spec-impact summary

The decisions that *changed* the spec materially:

- **D4 (Chat in v0):** add channel kind `0x0009`, frame types, capability entry.
- **D6 (TextInput):** add Input channel message type `0x07` in `20-wire-format-v0.md`.
- **D7 (Hybrid CBOR):** add a "Encoding profile" paragraph in `20-wire-format-v0.md`.
- **D11 (0-RTT):** add conditions in `21-state-machines.md`.
- **D5 (Recording):** add `docs/appendix-recording.md` (informative).

Other decisions match the existing doc text and just close the
open-question lists.
