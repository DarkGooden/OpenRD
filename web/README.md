# OpenRD Web Client — v0 scaffolding

This directory holds the placeholder web client. It is currently
just enough JavaScript to open a WebTransport connection to the
reference server, send a stub `ClientHello` Control frame, and log
what comes back.

There is no build step — the files are static HTML + ES modules.

## Serving locally

Any static file server works. With Python:

```sh
python3 -m http.server 8080
```

Then open `http://localhost:8080/` in a browser that supports
WebTransport (recent Chromium-based; Firefox shipping).

## What works today

- Opens a WebTransport session to the server URL in the input box.
- Opens the Control bidirectional stream.
- Sends a `ClientHello` frame with the protocol version, a client
  name, and an empty capabilities map.
- Logs incoming bytes from the server.

## What does NOT work yet

- The server doesn't parse the ClientHello yet — that's TODO in
  `crates/openrd-server/src/main.rs`.
- No `ServerHello` parsing on the client side.
- No video decode (Display channel not wired).
- No input forwarding (Input channel not wired).
- No auth, no resumption, no clipboard, no files, no chat.

## Self-signed certificate caveat

The reference server uses a self-signed cert for `localhost` in dev
mode. To make WebTransport accept it without a CA, you have to start
Chromium with:

```sh
google-chrome \
  --origin-to-force-quic-on=localhost:4443 \
  --ignore-certificate-errors-spki-list=<spki-hash-here>
```

(The SPKI hash is logged by the server at startup once that's
implemented.) Or use a proper CA-signed cert.
