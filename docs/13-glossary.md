# 13 — Glossary

> Status: Draft v0.1
> Last updated: 2026-05-13

Terms used throughout the OpenRD specification. Where a term has a
prior meaning in another protocol (RDP, VNC, QUIC, TLS), this glossary
uses the OpenRD meaning, which is usually but not always the same.

---

**AEAD.** Authenticated Encryption with Associated Data. The class of
ciphers (AES-GCM, ChaCha20-Poly1305) used by TLS 1.3 and QUIC for
both confidentiality and integrity.

**Auth backend.** A pluggable component invoked by the server to
validate a credential and return an authenticated identity plus a
permission level. Not part of the wire protocol; the protocol carries
credentials and identity, the auth backend decides.

**Capability.** A named, optionally-valued feature declaration
exchanged between client and server during capability negotiation
to determine what features are supported in this session.

**Capability negotiation.** The exchange of capability descriptors
between client and server immediately after the Control channel opens,
before any other channel is permitted. Specified in
[`22-capability-negotiation.md`](22-capability-negotiation.md).

**CBOR.** Concise Binary Object Representation (RFC 8949). The binary
encoding used for structured messages on the Control channel and for
manifests on the File channel.

**Channel.** A logical, typed stream of data between client and server.
Multiplexed onto QUIC streams. Examples: Control, Display, Input,
Clipboard, File. Defined in
[`11-channel-model.md`](11-channel-model.md).

**Channel ID.** A 32-bit unsigned value, unique within a session,
assigned by the side that opens the channel. Combined with the
**channel kind** it fully identifies a channel.

**Channel kind.** A 16-bit unsigned value identifying the *type* of
channel (Control, Display, etc.). Distinct from the channel ID, which
is a session-scoped instance handle.

**Client.** The endpoint that initiates a session and that
typically displays the remote desktop to a user.

**Connection.** The QUIC connection. One per session in v0. May span
multiple underlying network paths thanks to QUIC connection migration.

**Connection migration.** QUIC's ability to keep a connection alive
across changes of the client's IP address or port. OpenRD relies on
this for UC-5 (mobile) and UC-6 (kiosk).

**Consent prompt.** A Control-channel message routed to the
controlling user that requests authorization for a runtime action,
such as permission elevation from view-only to interactive.

**Control channel.** The mandatory channel (kind 0x0001) carrying
capability negotiation, channel open/close, session events, and
errors. See [`channel model`](11-channel-model.md).

**Credential.** Any of: a bearer token, a TLS client certificate, a
PIN, or another auth-backend-specific artifact presented by the client
to authenticate.

**Desktop session.** The graphical user session on the server host
that OpenRD captures and streams. May be a physical session
(real user logged in), a headless session (Xvfb / virtual compositor),
or a VM/container session.

**Direction.** A channel attribute: server-to-client, client-to-server,
or bidirectional.

**Display channel.** The channel (kind 0x0002) carrying encoded
video of the remote desktop. v0 codec is H.264 Baseline /
Constrained Baseline.

**Endpoint.** Either the client or the server. Used when stating a
requirement that applies to both.

**Frame.** A single picture of the desktop emitted by the encoder.
May be an IDR (keyframe) or a P-frame.

**Glyph cache.** A per-session, client-side cache of rasterized
font glyphs keyed by hash. The server may reference cache entries
in the Control channel to suppress the corresponding pixels in the
Display channel for crisper text. Optional in v0; recommended for
v1.

**IDR.** Instantaneous Decoder Refresh — an H.264 keyframe that
permits the decoder to start (or restart) without any prior frames.

**Input channel.** The channel (kind 0x0004) carrying keyboard,
mouse, and touch events from the client to the server.

**Instance ID.** See **channel ID**.

**Interactive.** A permission level granting the client the ability
to send Input events and Clipboard / File data. The strongest
permission level a v0 session can hold. Contrast **view-only**.

**Invitation token.** A short-lived, signed credential issued out of
band (typically by the server operator) that allows a one-time
session establishment. The default lifetime is 10 minutes.

**Manifest.** The metadata for a File-channel transfer: list of paths,
sizes, permissions, and a Merkle root hash of the per-chunk hashes.

**MCS.** Multipoint Communication Service, an ITU-T multimedia
conferencing layer used by Microsoft RDP. Not used by OpenRD;
mentioned in [`studies/rdp.md`](studies/rdp.md).

**NLA.** Network Level Authentication. Microsoft RDP's
authenticate-before-allocate-session model. OpenRD adopts the
*principle* but not the protocol.

**Operator.** The human or organization running an OpenRD server
deployment. Operators control the auth backend, network configuration,
and host machine; they are out of scope for protocol authentication
but are the user of the protocol's deployment surface.

**Peer.** Either endpoint, used when speaking generically about
either client or server. (When direction matters, "client" and
"server" are used.)

**Permission level.** The authorization granted to the client for the
current session. v0 levels are view-only and interactive. Permission
levels can be upgraded mid-session via a consent prompt; downgrades
take effect immediately.

**PIN pairing.** An authentication mode in which the server displays a
short numeric code (e.g., 9 digits) and the client enters it to
authenticate. Suitable for ad-hoc support scenarios (UC-2). The PIN
mixes into a key exchange so observing the PIN is not enough to
impersonate either side.

**Profile.** A pre-declared subset of OpenRD capabilities that an
implementation MUST support to claim conformance at a given level.
v0 defines a single profile: "OpenRD v0 Base."

**QUIC.** RFC 9000 — the transport protocol on which OpenRD is built.

**Resumption.** Re-attaching to an existing server-side session
after a transport-level disconnect, without re-authenticating.
Bounded by the resumption window (default 30 s, configurable).

**Resumption token.** A short-lived, single-use credential issued by
the server when a session enters READY, used during resumption to
prove the client's identity without a full auth roundtrip.

**Session.** The unit of OpenRD service from a client's perspective:
a desktop allocation, a set of channels, an authenticated identity,
and a permission level. A session may outlive any individual QUIC
connection (via resumption).

**Session ID.** A 128-bit opaque identifier for a session, allocated
by the server at session creation. Used during resumption.

**Server.** The endpoint hosting the desktop session being shared.
In v0, runs on Linux.

**Slice.** A horizontal band of an encoded video frame, sent as an
independent unit so a single packet loss damages at most one band.

**Stats channel.** The optional channel (kind 0x0008) carrying
periodic protocol-level telemetry.

**Stream.** A QUIC stream. A channel may use one or more streams.
Inside one stream, data is ordered and reliable; across streams, no
ordering guarantee applies.

**Transfer.** A single file-or-directory-tree movement on the File
channel, identified by its transfer (instance) ID.

**Transfer ID.** A 32-bit unsigned value, unique among in-flight
transfers, identifying a single transfer on the File channel.

**TPKT, T.120, T.123, T.125, T.128.** ITU-T standards underlying
Microsoft RDP. **Not** used by OpenRD. Mentioned in
[`studies/rdp.md`](studies/rdp.md).

**Token.** A short string of bytes presented by the client as
credential. May be an invitation token, a long-lived bearer token,
or a resumption token. Context distinguishes.

**v0.** The version this documentation specifies. A learning release.
Backwards compatibility begins at v1.

**Vendor extension.** A non-standard channel or message type
identified by a value in 0x8000–0xFFFF. Implementations MUST ignore
vendor extensions they do not understand.

**View-only.** A permission level granting the client the ability to
receive Display, Cursor, and Audio channels and read Stats, but NOT
to send Input or to read or write the remote Clipboard or File.

**WebTransport.** A W3C API providing QUIC streams to web pages.
OpenRD's primary browser transport.
