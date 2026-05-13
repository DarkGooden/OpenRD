# Case Study — Apache Guacamole

> Source material: guacamole.apache.org, github.com/apache/guacamole-server,
> github.com/apache/guacamole-client.

## Overview

Apache Guacamole is unusual: it is **not a remote-desktop protocol** in
the same sense as RDP or VNC. It is a *gateway* that translates between
several existing remote-desktop protocols (RDP, VNC, SSH, Kubernetes
exec, Telnet) on the back end, and a single, simple, browser-friendly
protocol (the "Guacamole protocol") on the front end.

The architecture is:

```
+--------+         +-------------+        +-----------------+
| Browser| <-----> | guacd (C)   | <----> | RDP/VNC/SSH host|
| (HTML5)|  Guac   | proxy/      | RDP/   |                 |
+--------+ protocol| translator  | VNC/   +-----------------+
                   +-------------+ SSH
```

Guacamole's value proposition is "no client install — just a browser."
This makes it a critical reference for OpenRD's web-client work even
though we are designing a single protocol rather than a translator.

## Wire format / transport

The Guacamole protocol on the wire is **text-based**: instructions are
comma-separated strings terminated by a semicolon, with explicit
length-prefixed string fields:

```
4.size,4.1024,3.768;       (resize canvas to 1024x768)
3.key,1.65,1.1;            (key event: keysym 65, pressed)
4.png,1.0,...png-data... ; (draw a PNG)
```

Each token is `<length>.<value>`. Crude, debuggable, and surprisingly
efficient to parse.

Transport is **WebSocket** between the browser and `guacd`. The
WebSocket carries the Guacamole protocol bidirectionally.

## Channel model

There isn't a multi-channel model in the QUIC/MCS sense. Everything —
display instructions, input, clipboard, audio, file transfer — is a
typed Guacamole instruction inside the single WebSocket stream.

This works for Guacamole because:

- The traffic mix is dominated by display instructions, which are
  small and frequent.
- File transfers are a recognized instruction type that the parser
  handles by streaming into the appropriate output.
- The translator on the back end already broke the data out of the
  source protocol's multi-channel model.

## Security model

- **WebSocket over TLS** between browser and guacd.
- **Authentication** is performed at the Java web-app layer
  (`guacamole`, not `guacd`): pluggable auth backends include LDAP,
  SAML, OIDC, header-based, file-based, JDBC.
- guacd itself does not authenticate users — it trusts the web app to
  do so before handing it a connection.

The split between `guacamole` (Java web app, handles auth and UI) and
`guacd` (C daemon, handles protocol translation) is a clean separation
of concerns but means deployments need both.

## Compression / codec

The display channel uses **drawing instructions** plus **PNG, JPEG, or
WebP image data** for non-primitive content. There is **no video
codec** in the Guacamole protocol — every frame, when video is being
played on the remote desktop, becomes a series of image updates.

This is the design's biggest weakness. Playing a video over Guacamole
saturates the bandwidth and burns CPU on both ends because each frame
becomes a fresh PNG.

A WebM (VP8/VP9) extension exists for streaming audio + video into the
session, but it's for specific use cases (Kubernetes exec output, etc.),
not for the desktop display channel.

## What's good

1. **Web client as the primary client.** No installs. This is OpenRD's
   target experience for UC-2 (Carlos the support tech on a Chromebook).
2. **Single transport (WebSocket).** Works through any firewall that
   allows HTTPS, which is essentially all of them.
3. **Text-based protocol is debuggable** — you can tcpdump and read
   the wire.
4. **Pluggable auth at the web-app layer** gives operators flexibility
   without bloating the protocol.
5. **Translation gateway pattern** is a clever way to deliver a uniform
   client experience over heterogeneous back ends. OpenRD doesn't
   replicate this, but it's an interesting design point.

## What's bad

1. **No video codec.** PNG-per-frame for video content is wasteful.
2. **Single-stream multiplexing.** Big file transfers compete with
   interactive responsiveness in the same WebSocket.
3. **Text framing is verbose.** OK for low-bandwidth desktop content,
   poor for high-bandwidth video.
4. **Latency is "good enough" for productivity but never sub-30 ms.**
   The translation hop adds delay; the text framing adds parsing
   overhead.
5. **Architecture requires two services** (guacamole + guacd) plus the
   back-end protocol's server. Operationally heavier than necessary.
6. **No native client option.** The protocol *can* be spoken by native
   clients but nobody does because Guacamole's whole point is "use a
   browser."

## What OpenRD should copy

1. **Web client as a first-class target**, designed from the start, not
   bolted on. Browser API constraints shape the protocol.
2. **WebSocket / WebTransport / WebRTC** as the browser-side transport,
   selected based on capability.
3. **Pluggable auth at the application layer**, not baked into the
   protocol. The protocol carries tokens; the operator's auth backend
   issues them.
4. **Debuggable wire format.** OpenRD's wire format will be binary, not
   text, but should have a clearly-documented byte layout that a
   knowledgeable engineer can inspect with Wireshark.

## What OpenRD should do differently

1. **Use a real video codec for the display channel** (H.264), not
   per-frame PNGs.
2. **Multi-channel multiplexing.** A big file transfer doesn't get to
   block the input stream.
3. **No translation layer.** OpenRD speaks one protocol end-to-end. A
   gateway *could* be written to translate OpenRD ↔ RDP for legacy
   environments, but the core protocol does not depend on it.
4. **No two-service split.** One server binary. The auth backend can be
   external but the server-to-client path is one process.

## References

- guacamole.apache.org
- guacamole.apache.org/doc/gug/guacamole-protocol.html
- github.com/apache/guacamole-server
- github.com/apache/guacamole-client
