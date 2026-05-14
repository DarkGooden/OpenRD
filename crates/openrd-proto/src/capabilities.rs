//! Capability advertisements and negotiation.
//!
//! See `docs/22-capability-negotiation.md`.
//!
//! Each peer advertises a [`Capabilities`] struct inside its
//! `ClientHello` / `ServerHello`. [`NegotiatedProfile::negotiate`]
//! computes the intersection both sides will use for the rest of
//! the session.

use ciborium::Value as Cbor;
use thiserror::Error;

/// What this peer supports.
#[derive(Debug, Clone)]
pub struct Capabilities {
    pub protocol_versions: Vec<u32>,
    pub profile: String,
    pub auth_methods: Vec<String>,
    pub display_codecs: Vec<String>,
    pub display_max_resolution: (u32, u32),
    pub display_max_fps: u32,
    pub audio_codecs: Vec<String>,
    pub clipboard_types: Vec<String>,
    pub file_max_concurrent: u32,
    pub file_max_size_per_transfer: u64,
    pub file_chunk_size_range: (u32, u32),
    pub clipboard_max_size: u32,
    pub resumption_window_seconds: u32,
    pub transport_features: Vec<String>,
    pub chat_enabled: bool,
    pub chat_max_attachment: u32,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            protocol_versions: vec![0],
            profile: "openrd-v0-base".into(),
            auth_methods: vec!["pin".into(), "token".into()],
            display_codecs: vec!["h264-baseline".into()],
            display_max_resolution: (1920, 1080),
            display_max_fps: 30,
            audio_codecs: vec!["opus".into()],
            clipboard_types: vec![
                "text/plain;charset=utf-8".into(),
                "image/png".into(),
            ],
            file_max_concurrent: 4,
            file_max_size_per_transfer: 16 * 1024 * 1024 * 1024,
            file_chunk_size_range: (4096, 4 * 1024 * 1024),
            clipboard_max_size: 64 * 1024 * 1024,
            resumption_window_seconds: 30,
            transport_features: vec!["quic-datagrams".into()],
            chat_enabled: true,
            chat_max_attachment: 1024 * 1024,
        }
    }
}

impl Capabilities {
    /// Serialize as a CBOR map per `docs/22-capability-negotiation.md`.
    pub fn to_cbor(&self) -> Cbor {
        let chat = Cbor::Map(vec![
            (key(1), Cbor::Bool(self.chat_enabled)),
            (key(2), uint(self.chat_max_attachment as u64)),
        ]);
        let res = Cbor::Array(vec![
            uint(self.display_max_resolution.0 as u64),
            uint(self.display_max_resolution.1 as u64),
        ]);
        let chunk_range = Cbor::Array(vec![
            uint(self.file_chunk_size_range.0 as u64),
            uint(self.file_chunk_size_range.1 as u64),
        ]);
        Cbor::Map(vec![
            (key(1), arr_u32(&self.protocol_versions)),
            (key(2), Cbor::Text(self.profile.clone())),
            (key(3), arr_text(&self.auth_methods)),
            (key(4), arr_text(&self.display_codecs)),
            (key(5), res),
            (key(6), uint(self.display_max_fps as u64)),
            (key(7), arr_text(&self.audio_codecs)),
            (key(8), arr_text(&self.clipboard_types)),
            (key(9), uint(self.file_max_concurrent as u64)),
            (key(10), uint(self.file_max_size_per_transfer)),
            (key(11), chunk_range),
            (key(12), uint(self.clipboard_max_size as u64)),
            (key(13), uint(self.resumption_window_seconds as u64)),
            (key(14), arr_text(&self.transport_features)),
            (key(16), chat),
        ])
    }

    /// Decode from a CBOR map. Missing or malformed entries fall back
    /// to `Default::default()` values so this is lenient on unknown
    /// peers.
    pub fn from_cbor(v: &Cbor) -> Self {
        let mut out = Self::default();
        let map = match v.as_map() {
            Some(m) => m,
            None => return out,
        };
        for (k, val) in map {
            let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
                Some(k) => k,
                None => continue,
            };
            match key {
                1 => out.protocol_versions = read_arr_u32(val),
                2 => if let Some(s) = val.as_text() { out.profile = s.into(); },
                3 => out.auth_methods = read_arr_text(val),
                4 => out.display_codecs = read_arr_text(val),
                5 => if let Some(t) = read_pair(val) { out.display_max_resolution = (t.0 as u32, t.1 as u32); },
                6 => if let Some(n) = read_u64(val) { out.display_max_fps = n as u32; },
                7 => out.audio_codecs = read_arr_text(val),
                8 => out.clipboard_types = read_arr_text(val),
                9 => if let Some(n) = read_u64(val) { out.file_max_concurrent = n as u32; },
                10 => if let Some(n) = read_u64(val) { out.file_max_size_per_transfer = n; },
                11 => if let Some(t) = read_pair(val) { out.file_chunk_size_range = (t.0 as u32, t.1 as u32); },
                12 => if let Some(n) = read_u64(val) { out.clipboard_max_size = n as u32; },
                13 => if let Some(n) = read_u64(val) { out.resumption_window_seconds = n as u32; },
                14 => out.transport_features = read_arr_text(val),
                16 => {
                    if let Some(m) = val.as_map() {
                        for (kk, vv) in m {
                            let ik = match kk.as_integer().and_then(|i| u64::try_from(i).ok()) {
                                Some(k) => k,
                                None => continue,
                            };
                            match ik {
                                1 => if let Some(b) = vv.as_bool() { out.chat_enabled = b; },
                                2 => if let Some(n) = read_u64(vv) { out.chat_max_attachment = n as u32; },
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// The single agreed-upon configuration for a session, computed from
/// `(client_caps, server_caps)` and stable for the session's lifetime.
#[derive(Debug, Clone)]
pub struct NegotiatedProfile {
    pub version: u32,
    pub profile: String,
    pub auth_methods: Vec<String>,
    pub display_codec: String,
    pub display_resolution: (u32, u32),
    pub display_max_fps: u32,
    pub audio_codec: Option<String>,
    pub clipboard_types: Vec<String>,
    pub file_max_concurrent: u32,
    pub file_max_size: u64,
    pub file_chunk_size_range: (u32, u32),
    pub clipboard_max_size: u32,
    pub resumption_window: u32,
    pub transport_features: Vec<String>,
    pub chat_enabled: bool,
}

#[derive(Debug, Error)]
pub enum NegotiationError {
    #[error("no overlapping protocol version")]
    NoVersionOverlap,
    #[error("no shared auth method")]
    NoAuthMethod,
    #[error("no shared display codec")]
    NoDisplayCodec,
}

impl NegotiatedProfile {
    pub fn negotiate(
        client: &Capabilities,
        server: &Capabilities,
    ) -> Result<Self, NegotiationError> {
        let version = client
            .protocol_versions
            .iter()
            .copied()
            .filter(|v| server.protocol_versions.contains(v))
            .max()
            .ok_or(NegotiationError::NoVersionOverlap)?;

        let auth_methods: Vec<String> = server
            .auth_methods
            .iter()
            .filter(|m| client.auth_methods.contains(m))
            .cloned()
            .collect();
        if auth_methods.is_empty() {
            return Err(NegotiationError::NoAuthMethod);
        }

        let display_codec = client
            .display_codecs
            .iter()
            .find(|c| server.display_codecs.contains(c))
            .cloned()
            .ok_or(NegotiationError::NoDisplayCodec)?;

        let audio_codec = client
            .audio_codecs
            .iter()
            .find(|c| server.audio_codecs.contains(c))
            .cloned();

        Ok(Self {
            version,
            profile: server.profile.clone(),
            auth_methods,
            display_codec,
            display_resolution: (
                client.display_max_resolution.0.min(server.display_max_resolution.0),
                client.display_max_resolution.1.min(server.display_max_resolution.1),
            ),
            display_max_fps: client.display_max_fps.min(server.display_max_fps),
            audio_codec,
            clipboard_types: server
                .clipboard_types
                .iter()
                .filter(|t| client.clipboard_types.contains(t))
                .cloned()
                .collect(),
            file_max_concurrent: client.file_max_concurrent.min(server.file_max_concurrent),
            file_max_size: client.file_max_size_per_transfer.min(server.file_max_size_per_transfer),
            file_chunk_size_range: (
                client.file_chunk_size_range.0.max(server.file_chunk_size_range.0),
                client.file_chunk_size_range.1.min(server.file_chunk_size_range.1),
            ),
            clipboard_max_size: client.clipboard_max_size.min(server.clipboard_max_size),
            resumption_window: client
                .resumption_window_seconds
                .min(server.resumption_window_seconds),
            transport_features: server
                .transport_features
                .iter()
                .filter(|f| client.transport_features.contains(f))
                .cloned()
                .collect(),
            chat_enabled: client.chat_enabled && server.chat_enabled,
        })
    }
}

// ---- CBOR helpers ------------------------------------------------------

fn key(n: u32) -> Cbor {
    Cbor::Integer(n.into())
}
fn uint(n: u64) -> Cbor {
    Cbor::Integer(n.into())
}
fn arr_u32(v: &[u32]) -> Cbor {
    Cbor::Array(v.iter().map(|&n| uint(n as u64)).collect())
}
fn arr_text(v: &[String]) -> Cbor {
    Cbor::Array(v.iter().map(|s| Cbor::Text(s.clone())).collect())
}
fn read_u64(v: &Cbor) -> Option<u64> {
    v.as_integer().and_then(|i| u64::try_from(i).ok())
}
fn read_arr_u32(v: &Cbor) -> Vec<u32> {
    v.as_array()
        .map(|arr| arr.iter().filter_map(|x| read_u64(x).map(|n| n as u32)).collect())
        .unwrap_or_default()
}
fn read_arr_text(v: &Cbor) -> Vec<String> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_text().map(|s| s.to_owned()))
                .collect()
        })
        .unwrap_or_default()
}
fn read_pair(v: &Cbor) -> Option<(u64, u64)> {
    let arr = v.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    Some((read_u64(&arr[0])?, read_u64(&arr[1])?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_default() {
        let c = Capabilities::default();
        let v = c.to_cbor();
        let back = Capabilities::from_cbor(&v);
        assert_eq!(c.protocol_versions, back.protocol_versions);
        assert_eq!(c.profile, back.profile);
        assert_eq!(c.auth_methods, back.auth_methods);
        assert_eq!(c.display_codecs, back.display_codecs);
        assert_eq!(c.display_max_resolution, back.display_max_resolution);
        assert_eq!(c.chat_enabled, back.chat_enabled);
        assert_eq!(c.chat_max_attachment, back.chat_max_attachment);
    }

    #[test]
    fn negotiation_default_succeeds() {
        let c = Capabilities::default();
        let s = Capabilities::default();
        let n = NegotiatedProfile::negotiate(&c, &s).unwrap();
        assert_eq!(n.version, 0);
        assert_eq!(n.display_codec, "h264-baseline");
        assert!(n.auth_methods.contains(&"pin".to_string()));
        assert!(n.chat_enabled);
    }

    #[test]
    fn negotiation_no_codec_overlap_fails() {
        let mut c = Capabilities::default();
        let s = Capabilities::default();
        c.display_codecs = vec!["av1".into()];
        assert!(matches!(
            NegotiatedProfile::negotiate(&c, &s),
            Err(NegotiationError::NoDisplayCodec)
        ));
    }
}
