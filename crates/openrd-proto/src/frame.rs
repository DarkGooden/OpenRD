//! Frame envelope parsing and serialization.
//!
//! Every channel's frames share the same outer envelope:
//!
//! ```text
//! +--------+--------+--------------+----------+
//! | ver:u8 | type:u8| length:u32   | payload  |
//! +--------+--------+--------------+----------+
//!         1 byte    1 byte         4 bytes LE   length bytes
//! ```
//!
//! The `type` byte is interpreted in the context of the channel kind
//! carrying the stream. See `docs/20-wire-format-v0.md`.

use crate::error::ParseError;

/// OpenRD protocol version. v0 = 0.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(pub u8);

impl ProtocolVersion {
    pub const V0: ProtocolVersion = ProtocolVersion(0);
}

/// Hard cap on frame payload size: 16 MiB.
pub const MAX_FRAME_LENGTH: usize = 16 * 1024 * 1024;

/// The fixed-length frame header (6 bytes).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub version: ProtocolVersion,
    pub frame_type: u8,
    pub length: u32,
}

impl FrameHeader {
    pub const SIZE: usize = 6;

    /// Parse a header from a byte slice. The slice must hold at least
    /// 6 bytes; bytes beyond the header are ignored.
    pub fn parse(buf: &[u8]) -> Result<Self, ParseError> {
        if buf.len() < Self::SIZE {
            return Err(ParseError::ShortHeader);
        }
        let version = ProtocolVersion(buf[0]);
        let frame_type = buf[1];
        let length = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);
        if (length as usize) > MAX_FRAME_LENGTH {
            return Err(ParseError::OversizedFrame(length));
        }
        Ok(Self { version, frame_type, length })
    }

    /// Serialize the header into the provided buffer.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        out.push(self.version.0);
        out.push(self.frame_type);
        out.extend_from_slice(&self.length.to_le_bytes());
    }
}

/// A parsed frame: header plus a borrowed payload slice.
#[derive(Debug)]
pub struct Frame<'a> {
    pub header: FrameHeader,
    pub payload: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Parse a complete frame (header + payload) from `buf`. Returns
    /// the frame and the number of bytes consumed.
    pub fn parse(buf: &'a [u8]) -> Result<(Self, usize), ParseError> {
        let header = FrameHeader::parse(buf)?;
        let end = FrameHeader::SIZE + header.length as usize;
        if buf.len() < end {
            return Err(ParseError::ShortPayload);
        }
        let payload = &buf[FrameHeader::SIZE..end];
        Ok((Self { header, payload }, end))
    }

    /// Encode `payload` with a header of the given frame_type and
    /// append to `out`.
    pub fn encode(frame_type: u8, payload: &[u8], out: &mut Vec<u8>) {
        let header = FrameHeader {
            version: ProtocolVersion::V0,
            frame_type,
            length: payload.len() as u32,
        };
        header.write_to(out);
        out.extend_from_slice(payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_empty_payload() {
        let mut buf = Vec::new();
        Frame::encode(0x42, &[], &mut buf);
        let (frame, n) = Frame::parse(&buf).unwrap();
        assert_eq!(n, FrameHeader::SIZE);
        assert_eq!(frame.header.frame_type, 0x42);
        assert_eq!(frame.header.length, 0);
        assert_eq!(frame.payload, &[] as &[u8]);
    }

    #[test]
    fn round_trip_with_payload() {
        let mut buf = Vec::new();
        let payload = b"hello, openrd";
        Frame::encode(0x01, payload, &mut buf);
        let (frame, n) = Frame::parse(&buf).unwrap();
        assert_eq!(n, FrameHeader::SIZE + payload.len());
        assert_eq!(frame.header.version, ProtocolVersion::V0);
        assert_eq!(frame.header.frame_type, 0x01);
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn rejects_short_header() {
        assert!(matches!(
            FrameHeader::parse(&[0, 1, 2]),
            Err(ParseError::ShortHeader)
        ));
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut hdr = [0u8; FrameHeader::SIZE];
        hdr[0] = 0; // version
        hdr[1] = 0; // type
        hdr[2..].copy_from_slice(&(MAX_FRAME_LENGTH as u32 + 1).to_le_bytes());
        assert!(matches!(
            FrameHeader::parse(&hdr),
            Err(ParseError::OversizedFrame(_))
        ));
    }
}
