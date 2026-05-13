//! Cursor channel: position updates and shape changes.
//!
//! Separate from Display so cursor latency isn't gated by video
//! frame intervals. See `docs/11-channel-model.md`.

use crate::error::ParseError;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CursorFrameType {
    CursorMove   = 0x01,
    CursorShape  = 0x02,
    CursorHidden = 0x03,
}

impl CursorFrameType {
    pub fn from_u8(v: u8) -> Result<Self, ParseError> {
        Ok(match v {
            0x01 => Self::CursorMove,
            0x02 => Self::CursorShape,
            0x03 => Self::CursorHidden,
            _    => return Err(ParseError::UnknownFrameType(v, "Cursor")),
        })
    }
}

// TODO: structs + parsers/serializers for each frame type.
