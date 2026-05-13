//! OpenRD wire-protocol types, framing, and channel definitions.
//!
//! This crate is the byte-level reference for the OpenRD v0 wire
//! format. The authoritative specification lives in `docs/`; in
//! particular:
//!
//! - `docs/20-wire-format-v0.md` — every frame layout.
//! - `docs/11-channel-model.md` — channel kinds.
//! - `docs/22-capability-negotiation.md` — Hello / capability schema.
//!
//! Nothing in this crate does I/O. It is a pure types + parse/serialize
//! library, suitable for embedding in both server and client.

pub mod frame;
pub mod channel;
pub mod error;
pub mod control;
pub mod input;
pub mod display;
pub mod cursor;
pub mod clipboard;
pub mod file;
pub mod audio;
pub mod chat;

pub use frame::{Frame, FrameHeader, ProtocolVersion, MAX_FRAME_LENGTH};
pub use channel::{ChannelKind, ChannelId};
pub use error::{ErrorCode, ParseError};

/// The protocol version this crate implements.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V0;

/// The ALPN identifier for OpenRD over QUIC / WebTransport.
///
/// v0 uses the literal `"openrd/v0"`. Formal IANA registration is
/// deferred to v1 per `docs/decisions.md` D20.
pub const ALPN: &[u8] = b"openrd/v0";

/// Default port for QUIC and the TLS-over-TCP fallback.
pub const DEFAULT_PORT: u16 = 443;

/// Default chunk size for file transfers (256 KiB).
pub const DEFAULT_FILE_CHUNK_SIZE: u32 = 256 * 1024;
