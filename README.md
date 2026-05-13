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

Pre-alpha. The protocol is in design. This repository currently contains only
documentation. See [`docs/`](docs/) for the design corpus, organized in four
tiers:

1. **Foundations** — goals, use cases, requirements, threat model
2. **Case studies** — what we learn from each existing system
3. **Synthesis** — architecture, channels, transport, glossary
4. **Pre-implementation** — wire format, state machines, capability negotiation

## License

Apache License 2.0 — see [LICENSE](LICENSE). The patent grant matters for a
protocol that is intended to be implemented by anyone.

## Contributing

Once the v0 spec lands, contributions to the spec and to reference
implementations will be welcomed. Until then, design feedback via issues is
the most useful contribution.
