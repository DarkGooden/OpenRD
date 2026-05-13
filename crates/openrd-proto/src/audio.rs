//! Audio channel: server playback audio.
//!
//! Codec: Opus 48 kHz, mono or stereo. May be carried over QUIC
//! datagrams when both peers support them (SHOULD per
//! `docs/decisions.md` D10), otherwise a unidirectional QUIC stream.

use crate::error::ParseError;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioFrameType {
    AudioParams = 0x01,
    AudioFrame  = 0x02,
}

impl AudioFrameType {
    pub fn from_u8(v: u8) -> Result<Self, ParseError> {
        Ok(match v {
            0x01 => Self::AudioParams,
            0x02 => Self::AudioFrame,
            _    => return Err(ParseError::UnknownFrameType(v, "Audio")),
        })
    }
}

/// Audio codec identifier. v0 defines only Opus.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioCodec {
    Opus = 0x01,
}
