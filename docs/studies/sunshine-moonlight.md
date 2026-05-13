# Case Study — Sunshine + Moonlight

> Source material: Moonlight protocol notes (community-maintained),
> Sunshine source, original NVIDIA GameStream reverse-engineering work.

## Overview

**Moonlight** is an open-source client that originated as a clean-room
reverse-engineering of NVIDIA's proprietary **GameStream** protocol,
which streamed games from a GeForce-equipped PC to NVIDIA Shield devices.
NVIDIA discontinued GameStream in 2023, leaving Moonlight without an
official server.

**Sunshine** is an open-source server, written from scratch, that speaks
the same protocol Moonlight expects. Together they form the de facto open
stack for low-latency game streaming over a LAN or fast WAN.

Even though OpenRD explicitly targets productivity rather than gaming,
Sunshine/Moonlight is the most relevant open-source prior art for the
*low-latency* parts of our design: how to stream encoded video, input,
and audio over an unreliable transport with minimum delay.

## Wire format / transport

The protocol is layered:

- **Pairing & control plane** — HTTP/HTTPS on TCP ports 47984/47989/48010
  for host discovery, capability listing, pairing (PIN-based key
  exchange), launching an "app", and quitting.
- **Video stream** — UDP via **ENet**, a small reliable-UDP library
  originally written for game networking. ENet provides ordered channels,
  reliable and unreliable delivery, and basic congestion handling.
- **Audio stream** — UDP via ENet, separate from video.
- **Input stream** — UDP/TCP encrypted with AES-CTR using a key
  established during pairing.

Default ports include 47998 (video), 47999 (control), 48000 (audio),
48010 (RTSP-style negotiation).

## Channel model

Conceptually multi-channel, but the channels are realized as **separate
UDP flows** (different ports) rather than streams within a single
transport. This is simple but creates problems with NAT, firewalls, and
QoS — every flow has to traverse independently.

Channels:

- Video (encoded H.264 / H.265 / AV1 frames as RTP-ish packets)
- Audio (Opus over RTP-ish)
- Input (gamepad, mouse, keyboard)
- Control (heartbeat, IDR keyframe requests, stats)

No clipboard. No file transfer. Not in scope for gaming.

## Security model

- **Pairing:** initial trust established via a PIN displayed on the
  server and entered on the client. PIN is mixed into the key exchange.
- **Encryption:** AES-CTR with a session key derived from the pairing
  exchange. Not TLS-based.
- **Authentication after pairing:** client certificate stored from the
  pairing handshake.

The pairing UX is genuinely good — much friendlier than typing usernames
and passwords on a TV remote. The crypto is reasonable but custom rather
than building on TLS, which makes it harder to audit.

## Compression / codec

- **Video:** H.264 or H.265 (HEVC); AV1 in newer versions. **Hardware
  encode on the server is effectively mandatory** for gaming latency
  targets (NVENC, AMF, QuickSync, VAAPI). Software encode is technically
  supported but rarely usable at gaming framerates.
- **Audio:** Opus at 48 kHz, stereo or 5.1.
- **Frame model:** Periodic IDR keyframes plus P-frames; the client can
  request an immediate IDR on packet loss to resync.

The video pipeline is engineered for *latency*, not for quality at a
fixed bitrate. Encoder is run with `tune=zerolatency`, very small GOP,
no B-frames, and frame slices spread across UDP packets so a single
dropped packet damages only a small region of the screen.

## What's good

1. **Sub-30 ms achievable.** On a LAN with hardware encode, end-to-end
   latency can be under 20 ms. This is the existence proof that
   sub-30 ms is achievable for OpenRD's LAN target.
2. **Latency-tuned encode settings** (zerolatency, slice-based frames,
   no B-frames) are exactly the recipe OpenRD's video channel should
   adopt.
3. **Frame slicing for loss tolerance** — each frame is split across
   multiple UDP packets representing different image slices, so a
   single packet loss damages a band rather than the whole frame. Smart
   and worth copying.
4. **IDR-on-demand from the client** is the right way to recover from
   loss without permanently degrading quality.
5. **PIN pairing UX** is a model for OpenRD's invitation flow.
6. **ENet's reliable-UDP** is a solid alternative to QUIC for projects
   that don't want a full TLS stack — though OpenRD has decided on QUIC.

## What's bad

1. **Separate UDP ports per channel** is a NAT/firewall nightmare. OpenRD
   gets multiplexing for free via QUIC streams; one port total.
2. **No clipboard, no files.** These are P0 for productivity (UC-1, UC-2,
   UC-3, UC-4) but Sunshine/Moonlight have no answer.
3. **Hardware encode dependency.** Software encode works in theory; in
   practice nobody uses it because gaming frame rates demand GPU encode.
   OpenRD targets CPU-only at a more relaxed frame rate.
4. **Custom crypto** rather than TLS. The protocol predates QUIC being
   widely usable; in 2026 there's no good reason to roll your own.
5. **Productivity ergonomics are poor.** Text rendering suffers because
   H.264 is lossy and there's no bitmap/glyph cache.
6. **No session resumption.** Each connect is a fresh negotiation.
7. **HTTP-based control plane** is a strange mix with the UDP video
   plane and produces awkward firewall rules.

## What OpenRD should copy

1. **Video encoder settings:** `tune=zerolatency`, slice-based frame
   layout, small/zero GOP, no B-frames, IDR on demand.
2. **Slice-per-packet loss tolerance** for the display channel.
3. **PIN-pairing UX** as one of the supported authentication methods
   (especially for UC-2: support technician invites customer).
4. **The basic shape of the streaming pipeline.** Sunshine's pipeline
   is well-engineered for latency; the patterns generalize.

## What OpenRD should do differently

1. **One transport (QUIC), many streams.** No per-channel UDP ports.
2. **TLS 1.3 (via QUIC), no custom crypto.**
3. **Add clipboard and file transfer as core channels.**
4. **Make CPU encode the default**, hardware encode an optimization.
   This will cap the protocol's frame rate ambition relative to
   Sunshine — but that is fine because we are not gaming.
5. **Productivity-grade text rendering.** Glyph cache + bitmap cache
   (RDP-style) ride alongside the video channel and override regions
   that the server has classified as text.
6. **Session identity that survives reconnects.**

## References

- Moonlight: github.com/moonlight-stream
- Sunshine: github.com/LizardByte/Sunshine
- Moonlight protocol notes: github.com/moonlight-stream/moonlight-common-c
- ENet: enet.bespin.org
