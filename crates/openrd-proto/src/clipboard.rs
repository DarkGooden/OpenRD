//! Clipboard channel: paste-pull bidirectional content transfer.
//!
//! Privacy model: the side that wants to paste requests content; the
//! other side responds. No eager push of clipboard content. See
//! `docs/03-threat-model.md` T-7.

use crate::error::ParseError;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ClipboardFrameType {
    OfferTypes     = 0x01,
    RequestContent = 0x02,
    Content        = 0x03,
    ContentChunk   = 0x04,
    ContentEnd     = 0x05,
}

impl ClipboardFrameType {
    pub fn from_u8(v: u8) -> Result<Self, ParseError> {
        Ok(match v {
            0x01 => Self::OfferTypes,
            0x02 => Self::RequestContent,
            0x03 => Self::Content,
            0x04 => Self::ContentChunk,
            0x05 => Self::ContentEnd,
            _    => return Err(ParseError::UnknownFrameType(v, "Clipboard")),
        })
    }
}

/// Hard cap on clipboard content payload. Larger transfers MUST use
/// the File channel.
pub const MAX_CONTENT_BYTES: usize = 64 * 1024 * 1024;
