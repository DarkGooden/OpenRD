//! File channel: chunked, hashed file and directory transfers.
//!
//! One File channel per active transfer. Chunks are SHA-256 hashed
//! at the manifest level; receiver verifies and re-requests on
//! mismatch. See `docs/studies/file-transfer.md`.

use crate::error::ParseError;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FileFrameType {
    StartTransfer  = 0x01,
    Manifest       = 0x02,
    Chunk          = 0x03,
    AckChunk       = 0x04,
    EndTransfer    = 0x05,
    CancelTransfer = 0x06,
}

impl FileFrameType {
    pub fn from_u8(v: u8) -> Result<Self, ParseError> {
        Ok(match v {
            0x01 => Self::StartTransfer,
            0x02 => Self::Manifest,
            0x03 => Self::Chunk,
            0x04 => Self::AckChunk,
            0x05 => Self::EndTransfer,
            0x06 => Self::CancelTransfer,
            _    => return Err(ParseError::UnknownFrameType(v, "File")),
        })
    }
}

/// `AckChunk.status` codes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AckChunkStatus {
    Ok           = 0x00,
    HashMismatch = 0x01,
    OutOfSpace   = 0x02,
    WriteError   = 0x03,
}

/// Default chunk size: 256 KiB.
pub const DEFAULT_CHUNK_SIZE: u32 = 256 * 1024;
/// Minimum chunk size: 4 KiB.
pub const MIN_CHUNK_SIZE: u32 = 4 * 1024;
/// Maximum chunk size: 4 MiB.
pub const MAX_CHUNK_SIZE: u32 = 4 * 1024 * 1024;
