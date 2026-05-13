# 01 — Use Cases

> Status: Draft v0.1
> Last updated: 2026-05-13

These are the concrete scenarios OpenRD must serve well. Each scenario names
a persona, the action they take, and the implicit requirements that fall out
of it. Whenever a design decision is ambiguous, run it against these
scenarios.

The personas are illustrative; the requirements they expose are not.

---

## UC-1 — Maria, the sysadmin, debugs a misbehaving Linux server

**Persona.** Maria is a senior Linux sysadmin at a mid-size company. She
mostly works from her Windows laptop and an iPad on the road. She
administers ~200 Linux VMs across two data centers.

**Scenario.** A monitoring alert fires at 02:14 AM. Maria opens her laptop,
connects to the affected VM through OpenRD, drops into a terminal, and
inspects logs. She decides to pull a 600 MB log bundle back to her laptop
for offline analysis. While she's at it, she copies a 40-line config snippet
from her local text editor into a remote `nano` session.

**Requirements exposed.**

- Connection establishment under ~2 seconds even over a modest residential
  uplink.
- Round-trip keystroke latency low enough that a touch-typist does not
  feel lag (< 60 ms over WAN, < 30 ms over LAN).
- File transfer of 600 MB without locking up the input channel. Maria must
  still be able to type while the file is moving.
- Clipboard transfer of plain text, bidirectional, immediate.
- Works from Windows (laptop) and iOS (iPad) clients.
- No dependency on cloud infrastructure she does not control.

## UC-2 — Carlos, the support technician, fixes a customer's Windows machine

**Persona.** Carlos works for an MSP (managed service provider). He
remote-supports ~80 small business customers. Most of his customer machines
run Windows; some run macOS. Customers are behind consumer NATs.

**Scenario.** A customer calls saying Outlook will not open. Carlos sends
the customer a one-time invitation link. The customer clicks the link,
which downloads and runs the OpenRD support client. Carlos sees the customer's
desktop in his browser, takes control of the mouse and keyboard, looks at
event viewer, copies a stack trace into his ticketing system, drops a small
diagnostic tool onto the customer's desktop, runs it, and pulls the
resulting report back.

**Requirements exposed.**

- The client must be runnable as a single small binary (no admin install)
  on Windows and macOS.
- Web-based viewer: Carlos works from a Chromebook, no native client.
- File transfer must support sub-MB executables and ~10 MB reports without
  drama.
- Clipboard must include rich text (event viewer entries) and images
  (screenshots).
- Some mechanism for one-time, time-limited access. (NB: the *invitation*
  workflow is a higher-layer concern, but the protocol must support
  short-lived bearer tokens.)
- The support side may be behind corporate proxy; transport must work over
  HTTPS-equivalent ports (443) for reachability.

## UC-3 — Anita, the remote developer, uses a beefy Linux dev box

**Persona.** Anita is a software engineer on a laptop with 16 GB RAM. Her
team provides each developer a Linux VM with 64 GB RAM and a fast CPU for
builds. Anita SSHes for terminal work but uses OpenRD when she needs a
graphical IDE, a browser running locally on the VM, or a database GUI.

**Scenario.** Anita opens her IDE on the remote VM. She edits code, runs
tests, copies error messages back to a Slack channel locally, drops design
mockups from her laptop into the project folder, and listens to a build
log narration through the remote machine's text-to-speech output.

**Requirements exposed.**

- Continuous use over an 8-hour workday without session drops, memory
  growth, or quality drift.
- Crisp text rendering. IDE fonts at 100% scale must be readable;
  anti-aliased subpixel rendering should survive the codec.
- Mouse precision sufficient for clicking on a 1-pixel scrollbar edge.
- Clipboard for plain text, rich text, and images (mockups).
- Drag-and-drop file transfer in both directions.
- Audio playback from server to client.
- Tolerates suspend/resume on the laptop side without re-authenticating
  for at least a configurable grace period.

## UC-4 — Dev, the embedded developer, uses OpenRD inside their own software

**Persona.** Dev is the author of a SaaS product whose customers occasionally
need help from Dev's support team. Dev wants to embed a "Get Live Help"
button in their product that, when clicked, opens an OpenRD session to a
support agent.

**Scenario.** A customer clicks "Get Live Help." Dev's software calls
into the OpenRD client SDK, which establishes a connection to Dev's
OpenRD server. The customer sees a small embedded view of the support
agent's screen, and the support agent sees the customer's screen. The
agent guides them, can take temporary control if the customer approves
each elevation, and disconnects when done.

**Requirements exposed.**

- A small, embeddable client SDK with a clean API (no global state, no
  required windowing system).
- Permission elevation: the protocol must distinguish "view-only" from
  "interactive" sessions and support runtime transitions between them with
  explicit consent.
- Branding-neutral: the protocol and SDK must not impose a UI.

## UC-5 — A mobile field tech operates a desktop tool from a phone

**Persona.** Lin is a field tech who occasionally needs to drive a
Windows-only desktop application from her Android phone while standing in
front of equipment.

**Scenario.** Lin opens the OpenRD app on her phone, connects to her
office desktop, and uses pinch-to-zoom, on-screen keyboard, and a virtual
trackpad to drive the desktop app. She occasionally screenshots the remote
desktop to share with a colleague on WhatsApp.

**Requirements exposed.**

- Touch input mapped to mouse events (tap → click, two-finger pan → scroll,
  pinch → zoom; the *semantic* mapping is the client's job, but the
  protocol must carry absolute pointer coordinates and modifier states).
- Adaptive bitrate for cellular networks.
- The video frame must be extractable as a still image client-side
  (no DRM-style restrictions).
- Battery-aware: the client must be able to throttle frame rate or pause
  the video stream while keeping input and clipboard alive.

## UC-6 — A kiosk on a flaky WAN holds a session for hours

**Persona.** Pat operates a fleet of retail kiosks. Each kiosk runs a thin
client connected to a central application server over a 4G modem with
intermittent connectivity.

**Scenario.** A kiosk briefly loses connectivity for 8 seconds, then comes
back. The session should resume on the same TCP/QUIC connection if
possible, on a new connection if not, without forcing the operator to
re-authenticate.

**Requirements exposed.**

- Session resumption across short network outages (≤ 60 s by default,
  configurable).
- Connection migration: when the underlying network path changes (IP
  change, WiFi → cellular), the session must not drop. This is a strong
  argument for QUIC.
- Server-side session persistence independent of any single TCP/QUIC
  connection.

---

## What these use cases collectively imply

Reading them together, the load-bearing requirements are:

1. **Multi-channel concurrency** — input, file, clipboard, audio, video
   all running independently without blocking each other.
2. **Connection migration and session resumption** — Cases 5 and 6 both
   need this; either forces QUIC or a custom UDP layer.
3. **A web client is mandatory, not optional** — Case 2 absolutely
   requires it, and the browser environment is the most constrained, so
   designing for it first prevents painting ourselves into a corner.
4. **File transfer is a peer of video, not a special case.** — Cases 1, 2,
   3, and 4 all use it on the critical path.
5. **Permission elevation and consent** are first-class operations, not
   afterthoughts. — Cases 2 and 4 both require runtime consent flows.
