//! Display channel: frame types for the encoded video stream.
//!
//! v0 codec: H.264 Baseline / Constrained Baseline.
//! See `docs/20-wire-format-v0.md` Display channel section.

use crate::error::ParseError;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DisplayFrameType {
    FrameHeader      = 0x01,
    FrameSlice       = 0x02,
    FrameEnd         = 0x03,
    StreamParameters = 0x04,
}

impl DisplayFrameType {
    pub fn from_u8(v: u8) -> Result<Self, ParseError> {
        Ok(match v {
            0x01 => Self::FrameHeader,
            0x02 => Self::FrameSlice,
            0x03 => Self::FrameEnd,
            0x04 => Self::StreamParameters,
            _    => return Err(ParseError::UnknownFrameType(v, "Display")),
        })
    }
}

/// Codec identifier in `StreamParameters`. v0 only defines H.264 Baseline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DisplayCodec {
    H264Baseline = 0x01,
}

/// Frame flag bits in `FrameHeader.flags`.
pub mod frame_flags {
    pub const IDR:               u8 = 0x01;
    pub const FINAL_BEFORE_PAUSE: u8 = 0x02;
}

// TODO: parse/write helpers for FrameHeader / FrameSlice / FrameEnd /
// StreamParameters. Layouts are in the spec.
