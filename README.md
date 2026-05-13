# OpenRD

**An open, modern remote-desktop protocol for IT, support, and remote work.**

OpenRD is a clean-slate, permissively-licensed remote-desktop protocol designed
for the way people actually work today: connecting to a remote machine to use
it, transfer files to and from it, share its clipboard, and control it with
keyboard and mouse — at sub-30ms interactive latency, with strong end-to-end
security, and with reference implementations for Linux servers and Windows /
macOS / Web / mobile clients.

OpenRD is **not** a wire-compatible reimplementation of Microsoft RDP. It is a
new protocol that draws lessons from RDP, VNC, SPICE, Sunshine/Moonlight,
RustDesk, and Apache Guacamole, while shedding the legacy baggage of T.128 /
MCS / X.224 and the gaming-specific tradeoffs of GameStream-style protocols.

## Why another protocol?

Today's open-source landscape forces a hard choice:

| Option                   | Open spec?            | Modern transport? | Productivity-focused? |
|--------------------------|-----------------------|-------------------|------------------------|
| Microsoft RDP (FreeRDP)  | Yes (MS-RDPBCGR, etc.)| No (TCP/T.128)    | Yes                    |
| VNC / RFB                | Yes (RFC 6143)        | No (TCP-only)     | Partially              |
| Sunshine + Moonlight     | Partially             | Yes (ENet/UDP)    | No (gaming)            |
| RustDesk                 | No (impl-defined)     | Yes               | Yes                    |
| Citrix HDX / VMware Blast| No (proprietary)      | Yes               | Yes                    |

OpenRD aims to be the missing entry: **open spec, modern transport,
productivity-focused.**

## Status

Pre-alpha. The protocol is in design and the reference implementation
is being scaffolded.

- **Specification:** all 21 open design questions are resolved (see
  [`docs/decisions.md`](docs/decisions.md)). The v0 wire format,
  channel model, state machines, and capability negotiation are
  specified in [`docs/`](docs/).
- **Reference server (`openrd-server`):** Cargo workspace scaffolded.
  QUIC endpoint with ALPN `openrd/v0`. Control channel handler,
  capture, encode, and input injection are TODO.
- **Reference web client (`web/`):** placeholder. Opens a WebTransport
  stream and sends a stub `ClientHello`. No video / input / auth yet.

### Building

The server requires Rust 1.75+ and a Linux build environment for
capture / injection (those paths aren't wired yet, so the workspace
builds on any platform Rust supports).

```sh
cargo build --workspace
cargo test  --workspace
```

To run the placeholder server:

```sh
RUST_LOG=openrd_server=info cargo run -p openrd-server
```

To serve the web client placeholder:

```sh
python3 -m http.server -d web 8080
```

### Repository layout

```
OpenRD/
├── Cargo.toml                 ← workspace
├── crates/
│   ├── openrd-proto/          ← wire-format types, framing, channel defs
│   └── openrd-server/         ← reference server (Linux)
├── web/                       ← placeholder web client (static)
├── docs/                      ← the specification
│   ├── 00-goals-and-non-goals.md
│   ├── 01-use-cases.md
│   ├── 02-requirements.md
│   ├── 03-threat-model.md
│   ├── 10-architecture-overview.md
│   ├── 11-channel-model.md
│   ├── 12-transport-choice.md
│   ├── 13-glossary.md
│   ├── 20-wire-format-v0.md
│   ├── 21-state-machines.md
│   ├── 22-capability-negotiation.md
│   ├── appendix-recording.md  ← informative
│   ├── decisions.md           ← log of 21 resolved design questions
│   └── studies/               ← case studies: RDP, VNC, Sunshine, RustDesk, SPICE, Guacamole, file-transfer
├── README.md
└── LICENSE                    ← Apache 2.0
```

## License

Apache License 2.0 — see [LICENSE](LICENSE). The patent grant matters for a
protocol that is intended to be implemented by anyone.

## Contributing

Once the v0 spec lands, contributions to the spec and to reference
implementations will be welcomed. Until then, design feedback via issues is
the most useful contribution.
