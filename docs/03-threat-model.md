# 03 — Threat Model

> Status: Draft v0.1
> Last updated: 2026-05-13

This document enumerates who might attack an OpenRD deployment, what
they want, and what the protocol must prevent. Threats are grouped by
the STRIDE categories:

- **S**poofing — impersonating a legitimate identity
- **T**ampering — modifying data in transit or at rest
- **R**epudiation — denying that an action occurred
- **I**nformation disclosure — leaking confidential data
- **D**enial of service — making the service unavailable
- **E**levation of privilege — gaining capabilities beyond what's granted

Each threat has an identifier (`T-<n>`), a description, an attacker
profile, and the mitigation OpenRD MUST or SHOULD provide.

---

## Trust boundaries

```
+----------------+        +-----------------+        +----------------+
|  Client device |  <-->  |  Network (WAN)  |  <-->  |  Server host   |
+----------------+        +-----------------+        +----------------+
       |                          |                          |
   user input                  attackers                  desktop session
   user clipboard               (passive +                 server kernel
   user files                    active)                    OS users
```

We assume:

- **Client device** is *as trusted as the user themselves*. If the
  client device is compromised, the user is compromised; OpenRD cannot
  fix this.
- **Server host** is *as trusted as its administrators*. If the server
  host is compromised, OpenRD cannot fix this either.
- **Network** is fully untrusted. Any packet may be observed, dropped,
  reordered, replayed, or modified. Attackers can run their own QUIC
  endpoints, present forged TLS certificates from any non-pinned CA,
  and operate as an on-path attacker (router, ISP, hotspot operator).

Everything that follows lives at the trust boundary between client and
server, on the network.

---

## Attacker profiles

| Profile      | Capability                                                    |
|--------------|---------------------------------------------------------------|
| **Passive**  | Reads all traffic on the wire. Cannot modify.                 |
| **Active**   | Reads, modifies, drops, reorders, replays traffic.            |
| **Insider**  | Has valid credentials to one tenant; tries to escalate.       |
| **Server-side malware** | Out of scope (see trust boundary).                 |
| **Client-side malware** | Out of scope (see trust boundary).                 |
| **Hostile peer**        | A buggy or malicious counterparty (client or server) trying to crash, exhaust, or compromise the other side via protocol messages. |

---

## Threats and mitigations

### Spoofing

#### T-1. Server impersonation (man-in-the-middle)

> An active attacker presents a forged TLS certificate and convinces the
> client it is the legitimate server.

- **Attacker profile.** Active network attacker.
- **Mitigation (MUST).** TLS 1.3 with one of:
  (a) **Certificate pinning** (client stores the server's public key on
  first connection and refuses any other key thereafter — TOFU model);
  (b) **CA-signed certificate** with explicit hostname verification;
  (c) **Out-of-band certificate distribution** (e.g., during invitation
  token issuance).
- **Mitigation (SHOULD).** Show the user the server's certificate
  fingerprint on first connection, in a way that can be verified out of
  band.

#### T-2. Client impersonation

> An attacker presents a stolen credential and connects as a legitimate
> user.

- **Attacker profile.** Active attacker who has obtained a token or key.
- **Mitigation (MUST).** Bearer tokens MUST be short-lived (default:
  invitation tokens ≤ 10 minutes; persistent tokens ≤ 24 hours unless
  explicitly extended). TLS client certificates MUST have explicit
  revocation paths. Audit log MUST record every authentication.
- **Mitigation (SHOULD).** Support hardware-bound credentials
  (WebAuthn / FIDO2, TPM-bound certs).

### Tampering

#### T-3. Input injection

> An attacker modifies keyboard or mouse events in transit to inject
> commands.

- **Attacker profile.** Active attacker, or compromised relay.
- **Mitigation (MUST).** All channels carry authenticated encryption
  (AEAD) inside TLS 1.3 / QUIC. Per-message authentication is provided
  by the transport AEAD.

#### T-4. File transfer corruption

> An attacker modifies a file in transit so the receiver stores a
> corrupted or maliciously substituted file.

- **Attacker profile.** Active attacker.
- **Mitigation (MUST).** Transport AEAD covers payload integrity.
- **Mitigation (SHOULD).** End-of-transfer SHA-256 hash verification
  with explicit rejection on mismatch.

### Repudiation

#### T-5. User denies a destructive action

> A user later claims they did not run a command or transfer a file.

- **Attacker profile.** Insider.
- **Mitigation (SHOULD).** Server-side audit log includes: session ID,
  authenticated identity, channel events (file open/close/transfer
  completion, permission elevations, session start/end). Out of scope
  for the protocol to *enforce* but the Control channel MUST emit the
  events the server needs to log them.

### Information disclosure

#### T-6. Passive eavesdropping on the wire

> An attacker on the network path records all traffic to extract
> keystrokes, clipboard content, files, or screen content.

- **Attacker profile.** Passive attacker.
- **Mitigation (MUST).** All channels encrypted via TLS 1.3 / QUIC. No
  cleartext fallback. Cipher suites limited to AEAD constructions.

#### T-7. Clipboard exfiltration via eager push

> A malicious server (or compromised one) reads the client's clipboard
> without user intent.

- **Attacker profile.** Hostile peer (server-side).
- **Mitigation (MUST).** Clipboard contents are *not* pushed eagerly.
  Each clipboard transfer is initiated by an explicit paste action on
  the receiving side. See F-3.5.

#### T-8. Side-channel leakage via traffic analysis

> An attacker observes encrypted traffic patterns to infer activity
> (e.g., typing patterns, application identity).

- **Attacker profile.** Passive attacker.
- **Mitigation (SHOULD).** Optional padding mode that pads input
  messages to a fixed size. Default off; opt-in for high-threat
  deployments because it increases bandwidth.
- **Note.** Full mitigation of traffic analysis is infeasible; OpenRD
  acknowledges this and does not claim it.

#### T-9. Cross-session data leak

> A new session sees stale data (clipboard, file handles, screen
> content) from a previous session.

- **Attacker profile.** Insider or hostile peer.
- **Mitigation (MUST).** Server MUST scrub clipboard buffer, file
  handles, and any session-scoped state when a session ends. Session
  resumption (NF-3.3) preserves state intentionally; resumption is only
  permitted under the same authenticated identity.

### Denial of service

#### T-10. Resource exhaustion via giant clipboard / file

> An attacker sends a 10 GB clipboard payload to OOM the peer.

- **Attacker profile.** Hostile peer.
- **Mitigation (MUST).** Per-channel size limits negotiated during
  capability exchange. Clipboard hard-capped at 64 MB (F-3.4) by
  default. Files streamed, not buffered.

#### T-11. Connection flooding

> Many half-open connections to the server.

- **Attacker profile.** Active attacker.
- **Mitigation (MUST).** QUIC handshake includes retry mechanism
  (address validation). Server SHOULD rate-limit unauthenticated
  handshakes.

#### T-12. Slow-drip or slowloris

> An attacker opens a channel and trickles data to keep server resources
> occupied.

- **Attacker profile.** Active attacker.
- **Mitigation (MUST).** Per-channel idle timeouts (default 30 s for
  Control, 5 s for Input). Misbehaving peers are disconnected.

### Elevation of privilege

#### T-13. View-only client takes input control

> A client granted view-only access tries to send input events.

- **Attacker profile.** Insider.
- **Mitigation (MUST).** Server enforces permission level per-message.
  Input messages from a view-only client are dropped and logged. The
  enforcement point is the server; clients MUST NOT be trusted to
  self-restrict.

#### T-14. Channel confusion

> An attacker sends a malformed packet that the server interprets as a
> different channel (e.g., input as file data).

- **Attacker profile.** Hostile peer.
- **Mitigation (MUST).** Channel multiplexing uses explicit, typed
  channel IDs allocated at channel-open time. Wire format MUST NOT allow
  cross-channel confusion (length-prefixed, channel-ID-tagged frames).

#### T-15. Downgrade attack

> An attacker forces a weaker cipher, older protocol version, or skipped
> auth step.

- **Attacker profile.** Active attacker.
- **Mitigation (MUST).** Capability negotiation MUST be authenticated
  by the TLS/QUIC handshake (i.e., it happens *after* the handshake, so
  the AEAD covers it). Servers MUST reject sessions where capability
  negotiation indicates weaker auth than the server's policy requires.

---

## Cryptographic primitives mandated by this model

- **TLS 1.3** as the transport security layer (directly, or as part of
  QUIC).
- **AEAD ciphers** only — AES-GCM and ChaCha20-Poly1305.
- **Ed25519 or ECDSA P-256** for server keys (pinning) and for
  invitation token signatures.
- **SHA-256** for file integrity hashes.
- **Argon2id** for any password-based KDF (parameters: 64 MB memory,
  3 iterations, 1 parallelism baseline).

---

## What this threat model does NOT cover

- Compromise of the client device (keyloggers, screen scrapers).
- Compromise of the server host (root on the box).
- Insider attacks by users with legitimate, full-privilege accounts.
- Supply-chain attacks against implementations.
- Side channels in the underlying CPU (Spectre-class).
- Physical access to either endpoint.

These are real threats, but OpenRD-the-protocol cannot mitigate them
and we deliberately do not claim to. Operators must defend the endpoints
themselves.

---

## Resolved questions

All open questions are resolved — see [`decisions.md`](decisions.md):
- D16: No additional PFS beyond TLS 1.3.
- D5: Session recording → informative appendix only.
- D17: Hybrid post-quantum KEM → defer to v1, track TLS 1.3 hybrid rollout.
