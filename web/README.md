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

- Opens a WebTransport session to `https://localhost:4443/openrd`,
  pinning the server's self-signed cert via `serverCertificateHashes`.
- Runs the full v0 scaffolding flow:
  1. `ClientHello` (with capabilities)
  2. `ServerHello` (parsed, fields printed)
  3. `AuthRequest` (method=pin, credential=the PIN you paste)
  4. `AuthResult` (status / permission / identity printed)
  5. `OpenChannel(Input, channel_id=1)`
  6. `OpenChannelAck`
  7. Unidirectional Input stream with: `'a'` down, `'a'` up, and a
     `TextInput` of `"olá, mundo 🌍 (web)"`

## How to run

1. Start the server (inside the Docker container):

   ```sh
   docker compose run --rm --service-ports dev cargo run -p openrd-server
   ```

   The server logs two things you need:

   ```
   ... cert_sha256_dotted=8a:be:a4:b7:...
   ... pin=283262283
   ```

2. Serve the web client (host-side, any static file server is fine):

   ```sh
   python -m http.server -d web 8080
   ```

3. Open `http://localhost:8080/` in a recent Chromium-based browser
   (WebTransport is shipped in Chrome / Edge / Brave; Firefox is
   rolling out). Paste the dotted-hex cert hash and the PIN, then
   click **Connect & run flow**.

## What does NOT work yet

- Video decode (Display channel not wired — see M5).
- Live keyboard/mouse forwarding from the browser (only the canned
  test events are sent).
- No clipboard / files / chat / audio.
- Resumption isn't implemented.

## Self-signed cert: why the hash, not the cert

WebTransport's `serverCertificateHashes` option lets a browser
trust a specific self-signed cert without having to import a CA.
The browser hashes the cert it receives and checks the SHA-256
matches one in the list. The server prints the hash on startup so
you can paste it in.
