//! Control channel: frame types and CBOR message schemas.
//!
//! See `docs/20-wire-format-v0.md` Control channel frames section.
//!
//! Each Control frame's payload is CBOR. v0 uses Preferred
//! Serialization (RFC 8949 §4.1) for normal Control messages.
//! Signed structures (resumption tokens, invitation tokens) use
//! Deterministic Encoding (§4.2).

use serde::{Deserialize, Serialize};

/// Numeric frame type tags for Control-channel frames.
///
/// These are the `type` byte in the outer frame envelope.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlFrameType {
    ClientHello       = 0x01,
    ServerHello       = 0x02,
    AuthRequest       = 0x03,
    AuthChallenge     = 0x04,
    AuthResult        = 0x05,
    OpenChannel       = 0x06,
    OpenChannelAck    = 0x07,
    CloseChannel      = 0x08,
    ConsentRequest    = 0x09,
    ConsentResponse   = 0x0A,
    SessionEvent      = 0x0B,
    Error             = 0x0C,
    RequestKeyframe   = 0x0D,
    Ping              = 0x0E,
    Pong              = 0x0F,
    Stats             = 0x10,
    SessionResume     = 0x11,
    SessionResumed    = 0x12,
}

/// `ClientHello` (type 0x01). First Control frame sent by the client.
///
/// CBOR map keyed by short integers — see the spec.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientHello {
    #[serde(rename = "1")] pub protocol_version: u32,
    #[serde(rename = "2")] pub client_name: String,
    #[serde(rename = "3")] pub capabilities: ciborium::Value,
    #[serde(rename = "4", skip_serializing_if = "Option::is_none", default)]
    pub session_id_hint: Option<serde_bytes::ByteBuf>,
}

/// `ServerHello` (type 0x02). First Control frame sent by the server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerHello {
    #[serde(rename = "1")] pub protocol_version: u32,
    #[serde(rename = "2")] pub server_name: String,
    #[serde(rename = "3")] pub capabilities: ciborium::Value,
    #[serde(rename = "4")] pub session_id: serde_bytes::ByteBuf,
    #[serde(rename = "5")] pub server_time: u64,
}

// TODO: AuthRequest, AuthChallenge, AuthResult, OpenChannel,
// OpenChannelAck, CloseChannel, ConsentRequest, ConsentResponse,
// SessionEvent, Error, RequestKeyframe, Ping, Pong, Stats,
// SessionResume, SessionResumed.
//
// The structures are specified in docs/20-wire-format-v0.md; mapping
// them to `serde` structs is straightforward. Left as stubs while the
// reference server is being built up.

/// Encode a Control message into a CBOR byte vector.
///
/// Uses ciborium's default (Preferred) serialization. For signed
/// structures, use [`encode_deterministic`] instead.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, ciborium::ser::Error<std::io::Error>> {
    let mut out = Vec::new();
    ciborium::ser::into_writer(value, &mut out)?;
    Ok(out)
}

/// Decode a Control message from CBOR bytes.
pub fn decode<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
) -> Result<T, ciborium::de::Error<std::io::Error>> {
    ciborium::de::from_reader(bytes)
}

// TODO: `encode_deterministic` — emit Deterministic CBOR for
// resumption / invitation tokens. ciborium does not yet expose a
// canonical-encoding mode; we will likely either fork-encode or use
// a different crate (`coset`, `serde_cbor_2`) for signed paths.
