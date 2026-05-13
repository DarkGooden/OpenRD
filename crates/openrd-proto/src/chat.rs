//! Chat channel: real-time text chat between session peers.
//!
//! Optional. Decided in `docs/decisions.md` D4. Scope is deliberately
//! small in v0: text messages, typing indicators, small inline
//! attachments. No edit/delete, no read receipts, no threading.

use crate::error::ParseError;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ChatFrameType {
    ChatMessage     = 0x01,
    TypingIndicator = 0x02,
    ChatAttachment  = 0x03,
}

impl ChatFrameType {
    pub fn from_u8(v: u8) -> Result<Self, ParseError> {
        Ok(match v {
            0x01 => Self::ChatMessage,
            0x02 => Self::TypingIndicator,
            0x03 => Self::ChatAttachment,
            _    => return Err(ParseError::UnknownFrameType(v, "Chat")),
        })
    }
}

/// `ChatMessage` CBOR payload.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMessage {
    #[serde(rename = "1")] pub msg_id: u64,
    #[serde(rename = "2")] pub sender: String,
    #[serde(rename = "3")] pub body: String,
    #[serde(rename = "4")] pub ts_ms: u64,
}

/// `TypingIndicator` CBOR payload.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TypingIndicator {
    #[serde(rename = "1")] pub sender: String,
    #[serde(rename = "2")] pub state: String,  // "start" | "stop"
}

/// Inline attachment payload. Hard cap 1 MiB. Larger files use the
/// File channel and reference by path/transfer-ID in a ChatMessage.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatAttachment {
    #[serde(rename = "1")] pub msg_id: u64,
    #[serde(rename = "2")] pub sender: String,
    #[serde(rename = "3")] pub mime: String,
    #[serde(rename = "4", default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(rename = "5")] pub bytes: serde_bytes::ByteBuf,
}

pub const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024;
