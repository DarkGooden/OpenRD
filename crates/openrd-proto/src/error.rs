//! Wire-level error codes and parse errors.
//!
//! The numeric `ErrorCode` is the value that appears in the `code`
//! field of a Control-channel `Error` frame. See the table in
//! `docs/20-wire-format-v0.md`.

use thiserror::Error;

/// Standard OpenRD error codes. Values in `0x8000..=0xFFFE` are
/// reserved for vendor extensions. `0xFFFF` is `VendorDefined` —
/// used together with an out-of-band vendor-specific code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ErrorCode {
    Ok                      = 0x0000,
    InvalidFrame            = 0x0001,
    UnsupportedVersion      = 0x0002,
    Unauthenticated         = 0x0003,
    AuthFailed              = 0x0004,
    PermissionDenied        = 0x0005,
    UnknownChannelKind      = 0x0006,
    ChannelLimitExceeded    = 0x0007,
    ResourceExhausted       = 0x0008,
    RateLimited             = 0x0009,
    SessionExpired          = 0x000A,
    ResumptionRejected      = 0x000B,
    Internal                = 0x000C,
    NotImplemented          = 0x000D,
    ConsentDenied           = 0x000E,
    InvalidParameter        = 0x000F,
    VendorDefined           = 0xFFFF,
}

impl ErrorCode {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0x0000 => Self::Ok,
            0x0001 => Self::InvalidFrame,
            0x0002 => Self::UnsupportedVersion,
            0x0003 => Self::Unauthenticated,
            0x0004 => Self::AuthFailed,
            0x0005 => Self::PermissionDenied,
            0x0006 => Self::UnknownChannelKind,
            0x0007 => Self::ChannelLimitExceeded,
            0x0008 => Self::ResourceExhausted,
            0x0009 => Self::RateLimited,
            0x000A => Self::SessionExpired,
            0x000B => Self::ResumptionRejected,
            0x000C => Self::Internal,
            0x000D => Self::NotImplemented,
            0x000E => Self::ConsentDenied,
            0x000F => Self::InvalidParameter,
            _      => Self::VendorDefined,
        }
    }
}

/// Errors that can occur while parsing a frame off the wire.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("frame header too short (need 6 bytes)")]
    ShortHeader,

    #[error("frame payload truncated")]
    ShortPayload,

    #[error("frame length {0} exceeds maximum 16 MiB")]
    OversizedFrame(u32),

    #[error("CBOR decode failed: {0}")]
    Cbor(String),

    #[error("unknown frame type {0:#x} for channel {1}")]
    UnknownFrameType(u8, &'static str),

    #[error("invalid value for field {field}: {detail}")]
    InvalidField { field: &'static str, detail: String },
}
