//! Input channel: frame types for keyboard, mouse, touch, and text.
//!
//! See `docs/20-wire-format-v0.md` Input channel section.

use crate::error::ParseError;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InputFrameType {
    KeyEvent       = 0x01,
    PointerMove    = 0x02,
    PointerButton  = 0x03,
    PointerWheel   = 0x04,
    TouchEvent     = 0x05,
    SyntheticBatch = 0x06,
    TextInput      = 0x07,
}

impl InputFrameType {
    pub fn from_u8(v: u8) -> Result<Self, ParseError> {
        Ok(match v {
            0x01 => Self::KeyEvent,
            0x02 => Self::PointerMove,
            0x03 => Self::PointerButton,
            0x04 => Self::PointerWheel,
            0x05 => Self::TouchEvent,
            0x06 => Self::SyntheticBatch,
            0x07 => Self::TextInput,
            _ => return Err(ParseError::UnknownFrameType(v, "Input")),
        })
    }
}

/// Modifier key mask. See spec; bit 0 = Shift, bit 1 = Ctrl, etc.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Modifiers(pub u32);

impl Modifiers {
    pub const SHIFT:       u32 = 0x0001;
    pub const CTRL:        u32 = 0x0002;
    pub const ALT:         u32 = 0x0004;
    pub const META:        u32 = 0x0008;
    pub const ALTGR:       u32 = 0x0010;
    pub const CAPS_LOCK:   u32 = 0x0020;
    pub const NUM_LOCK:    u32 = 0x0040;
    pub const SCROLL_LOCK: u32 = 0x0080;
}

/// `KeyEvent` (type 0x01). Fixed 13-byte payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub keysym: u32,
    pub scancode: u32,
    pub modifiers: Modifiers,
    /// bit 0: down (1=pressed, 0=released); bit 1: repeat
    pub flags: u8,
}

impl KeyEvent {
    pub const SIZE: usize = 13;

    pub fn down(&self) -> bool { self.flags & 0x01 != 0 }
    pub fn is_repeat(&self) -> bool { self.flags & 0x02 != 0 }

    pub fn parse(buf: &[u8]) -> Result<Self, ParseError> {
        if buf.len() < Self::SIZE {
            return Err(ParseError::ShortPayload);
        }
        Ok(Self {
            keysym:    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            scancode:  u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            modifiers: Modifiers(u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]])),
            flags:     buf[12],
        })
    }

    pub fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.keysym.to_le_bytes());
        out.extend_from_slice(&self.scancode.to_le_bytes());
        out.extend_from_slice(&self.modifiers.0.to_le_bytes());
        out.push(self.flags);
    }
}

/// `TextInput` (type 0x07). UTF-8 text length-prefixed by u32.
///
/// Used for mobile keyboards, IMEs (committed text only), emoji,
/// voice-to-text. See `docs/decisions.md` D6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextInput {
    pub text: String,
}

impl TextInput {
    /// Maximum text length per TextInput message: 64 KiB.
    pub const MAX_BYTES: usize = 65_536;

    pub fn parse(buf: &[u8]) -> Result<Self, ParseError> {
        if buf.len() < 4 {
            return Err(ParseError::ShortPayload);
        }
        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if len > Self::MAX_BYTES {
            return Err(ParseError::InvalidField {
                field: "TextInput.text.length",
                detail: format!("{len} exceeds 64 KiB cap"),
            });
        }
        if buf.len() < 4 + len {
            return Err(ParseError::ShortPayload);
        }
        let text = std::str::from_utf8(&buf[4..4 + len])
            .map_err(|e| ParseError::InvalidField {
                field: "TextInput.text",
                detail: format!("invalid UTF-8: {e}"),
            })?
            .to_owned();
        Ok(Self { text })
    }

    pub fn write_to(&self, out: &mut Vec<u8>) {
        let bytes = self.text.as_bytes();
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
}

// TODO: PointerMove, PointerButton, PointerWheel, TouchEvent,
// SyntheticBatch. Layouts are in docs/20-wire-format-v0.md.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_round_trip() {
        let original = TextInput { text: "olá, mundo 🌍".into() };
        let mut buf = Vec::new();
        original.write_to(&mut buf);
        let parsed = TextInput::parse(&buf).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn key_event_round_trip() {
        let original = KeyEvent {
            keysym: 0x0041,
            scancode: 0x001E,
            modifiers: Modifiers(Modifiers::SHIFT),
            flags: 0x01,
        };
        let mut buf = Vec::new();
        original.write_to(&mut buf);
        let parsed = KeyEvent::parse(&buf).unwrap();
        assert_eq!(parsed, original);
        assert!(parsed.down());
        assert!(!parsed.is_repeat());
    }
}
