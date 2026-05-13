//! Channel kinds and instance IDs.
//!
//! See `docs/11-channel-model.md`. The kind is a stable 16-bit
//! constant defined by the spec; the ID is a per-session 32-bit
//! handle assigned by whichever side opens the channel.

/// A channel kind: the *type* of a channel.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChannelKind(pub u16);

impl ChannelKind {
    pub const CONTROL:   ChannelKind = ChannelKind(0x0001);
    pub const DISPLAY:   ChannelKind = ChannelKind(0x0002);
    pub const CURSOR:    ChannelKind = ChannelKind(0x0003);
    pub const INPUT:     ChannelKind = ChannelKind(0x0004);
    pub const CLIPBOARD: ChannelKind = ChannelKind(0x0005);
    pub const FILE:      ChannelKind = ChannelKind(0x0006);
    pub const AUDIO:     ChannelKind = ChannelKind(0x0007);
    pub const STATS:     ChannelKind = ChannelKind(0x0008);
    pub const CHAT:      ChannelKind = ChannelKind(0x0009);

    /// True if this kind is in the vendor-extension range
    /// (`0x8000..=0xFFFF`). The protocol guarantees that peers must
    /// ignore vendor kinds they do not understand.
    pub fn is_vendor(self) -> bool {
        self.0 >= 0x8000
    }

    /// Human-readable name for logs and debugging.
    pub fn name(self) -> &'static str {
        match self {
            Self::CONTROL   => "Control",
            Self::DISPLAY   => "Display",
            Self::CURSOR    => "Cursor",
            Self::INPUT     => "Input",
            Self::CLIPBOARD => "Clipboard",
            Self::FILE      => "File",
            Self::AUDIO     => "Audio",
            Self::STATS     => "Stats",
            Self::CHAT      => "Chat",
            _ if self.is_vendor() => "Vendor",
            _ => "Unknown",
        }
    }
}

/// A per-session channel instance identifier.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChannelId(pub u32);

/// Permission level required to operate a channel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PermissionLevel {
    /// Receive Display / Cursor / Audio, read Stats. No Input,
    /// Clipboard, or File.
    ViewOnly,
    /// Full session: all channels allowed (subject to consent
    /// for elevation when transitioning from ViewOnly).
    Interactive,
}
