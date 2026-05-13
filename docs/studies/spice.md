# Case Study — SPICE

> Source material: spice-space.org/spice-protocol.html;
> github.com/freedesktop/spice / spice-gtk / spice-server.

## Overview

SPICE (Simple Protocol for Independent Computing Environments) is the
remote-display protocol used in the QEMU/KVM ecosystem, originating at
Qumranet (acquired by Red Hat in 2008) and open-sourced shortly after.
SPICE is unusual among remote-display protocols because it is designed
to be *integrated with the hypervisor*, not just to scrape a finished
framebuffer.

A SPICE server runs inside QEMU; a paravirtualized graphics driver
(`qxl`, or more recently `virtio-gpu`) in the guest hands SPICE a stream
of drawing commands and surface buffers directly, instead of forcing
SPICE to detect changes by polling the framebuffer.

## Wire format / transport

- **TCP** for each channel (default ports 5900-range; one per channel).
- **TLS** support is optional but standard.
- **Per-channel TCP connections** — each logical channel uses its own
  TCP socket rather than multiplexing.
- **Custom binary framing** with little-endian fixed-width header
  fields. Cleaner than RDP's ASN.1 mess.

## Channel model

This is where SPICE shines and where its design is most relevant to
OpenRD. SPICE has a **rich, well-typed channel model** with separate
channels for separate concerns:

- **Main** — session control, mouse mode (server/client), agent
  forwarding.
- **Display** — drawing commands and image surfaces.
- **Inputs** — keyboard, mouse, tablet.
- **Cursor** — separate from display, so cursor moves don't require
  a full display update.
- **Playback** — server-to-client audio.
- **Record** — client-to-server audio (microphone).
- **Smartcard** — smart-card redirection.
- **USB redirection** — over USB-over-IP-ish protocol.
- **Tunnel** — generic TCP tunnel through the session.
- **Port** — serial port forwarding.
- **WebDAV** — for file shares (via the SPICE WebDAV agent).

The separation between **Display** and **Cursor** is especially clever.
Cursor movements happen at 100+ Hz on a desktop; encoding them inside the
main display stream would either spam the encoder or force the cursor
to lag behind the mouse. SPICE renders the cursor sprite client-side
from cursor-channel data, so cursor latency tracks the input channel,
not the display channel.

## Security model

- TLS for each channel when enabled.
- A per-session **ticket** (a short-lived shared secret) authenticates
  the client; the ticket is delivered out of band via libvirt or a
  management API.
- Newer versions support SASL.

The model is "the orchestrator owns the auth; SPICE just checks the
ticket." For a VM-management context this is sensible; for a
standalone remote-desktop tool it's not enough.

## Compression / codec

SPICE's display channel is **command-oriented**, not pixel-oriented:

- The guest GPU driver (qxl) sends drawing primitives:
  `DRAW_COPY`, `DRAW_FILL`, `DRAW_OPAQUE`, `DRAW_BLEND`, `DRAW_TEXT`, etc.
- For image content, SPICE sends bitmap surfaces with per-surface
  compression: GLZ (a SPICE-specific LZ variant tuned for screen
  content), LZ, JPEG (for photo-like content), Quic (SPICE's
  predictor-based image codec).
- For video-like content (detected by streaming heuristics), SPICE
  switches to a **stream** mode where the region is encoded with
  MJPEG or H.264 (in later versions).

The hybrid model is genuinely impressive on a real desktop: text and
window chrome go through the drawing-primitive path with near-perfect
fidelity, while video playback regions get codec'd.

## What's good

1. **Channel separation is excellent.** Cursor-as-its-own-channel is
   a particularly good idea.
2. **Hybrid codec model** — drawing primitives where applicable, image
   codecs for static bitmaps, video codecs for motion regions — is the
   right architecture for productivity workloads. SPICE detects video
   regions automatically and switches modes.
3. **Tunnel / Port / WebDAV channels** show the value of letting
   applications open arbitrary auxiliary channels through the same
   authenticated session.
4. **Per-channel TLS** is a clean security model.
5. **Hypervisor integration** is the right design for VM use cases —
   no framebuffer-scraping latency.
6. **Cleaner wire format than RDP.**

## What's bad

1. **One TCP connection per channel** = head-of-line blocking per channel
   plus a NAT and firewall headache. With many channels open, the
   connection count is annoying.
2. **Requires hypervisor / paravirtualized GPU driver** to get the
   command-stream benefits. On bare metal Linux desktops SPICE has to
   fall back to framebuffer scraping, where it offers no advantage
   over VNC or RDP.
3. **Limited deployment outside the KVM ecosystem.** No widely-deployed
   Windows server, no real browser client.
4. **Auth model is too tied to libvirt-style orchestration.** Standalone
   use is painful.
5. **Performance over WAN is mediocre.** SPICE was tuned for LAN/data
   center scenarios; on real internet links it shows.
6. **Specification is incomplete in places** and the source is the real
   reference.

## What OpenRD should copy

1. **Separate cursor channel.** Cursor moves do not go through the
   display encoder.
2. **The general idea of a hybrid codec model**, but adapted to our
   "H.264 only in v0" decision: the display channel is H.264, but the
   Control channel carries glyph and bitmap-cache references that the
   client overlays *on top of* the H.264 frame. This gives us
   SPICE-quality text rendering without inventing new video codecs.
3. **Optional auxiliary channels** for app-specific extensions
   (the equivalent of SPICE's Tunnel/Port channels). Defer the
   specification of these to v1.
4. **Per-channel concerns:** input, display, cursor, audio, files,
   clipboard, control. The set is informed by SPICE.

## What OpenRD should do differently

1. **One transport, many streams (QUIC).** Not one TCP socket per
   channel.
2. **No hypervisor coupling.** OpenRD is a remote-desktop protocol for
   real machines; framebuffer capture is the baseline path.
3. **First-class authentication** — token, mTLS, PIN — not "ask the
   orchestrator."
4. **Single video codec in v0** (H.264). SPICE's many image codecs are
   a maintenance burden for unclear benefit on modern CPUs.

## References

- spice-space.org/spice-protocol.html
- github.com/freedesktop/spice
- "SPICE for Newbies": spice-space.org/spice-for-newbies.html
