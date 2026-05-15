//! Control channel handler — v0 scaffolding (hello exchange only).
//!
//! Reads a `ClientHello` from the Control bidirectional stream,
//! validates the protocol version, and replies with a `ServerHello`.
//! Authentication, channel dispatch, ping/keepalive, and the rest of
//! the Control loop are TODO.

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use ciborium::Value as Cbor;
use openrd_proto::control::ControlFrameType;
use openrd_proto::{
    Capabilities, ErrorCode, Frame, FrameHeader, NegotiatedProfile, ProtocolVersion,
    MAX_FRAME_LENGTH,
};
use quinn::{RecvStream, SendStream};
use tracing::{info, warn};

/// Run the hello exchange and PIN authentication on the Control
/// bidirectional stream.
///
/// On success the function returns; the rest of the Control loop
/// (channel dispatch, keepalive, etc.) is not yet implemented and
/// the caller will close the connection.
pub async fn handle_control_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    remote: SocketAddr,
    expected_pin: &str,
) -> Result<()> {
    // Read one full frame off the wire.
    let frame_bytes = read_full_frame(&mut recv).await?;
    let (frame, _) = Frame::parse(&frame_bytes)?;

    if frame.header.version != ProtocolVersion::V0 {
        bail!("unsupported protocol version {:?}", frame.header.version);
    }
    if frame.header.frame_type != ControlFrameType::ClientHello as u8 {
        bail!(
            "expected ClientHello (0x01), got {:#04x}",
            frame.header.frame_type
        );
    }

    let hello: Cbor = ciborium::de::from_reader(frame.payload)
        .context("decode ClientHello CBOR")?;
    let (proto_v, client_name, client_caps) = parse_client_hello(&hello)?;
    info!(
        %remote,
        protocol_version = proto_v,
        client_name = %client_name,
        "received ClientHello"
    );

    // Build server capabilities and try negotiation; if it fails we
    // still send a ServerHello (the client will detect the mismatch)
    // but log the rejection reason here.
    let server_caps = Capabilities::default();
    match NegotiatedProfile::negotiate(&client_caps, &server_caps) {
        Ok(profile) => {
            info!(
                %remote,
                display_codec = %profile.display_codec,
                auth_methods = ?profile.auth_methods,
                chat = profile.chat_enabled,
                "negotiated profile"
            );
        }
        Err(e) => {
            warn!(%remote, "capability negotiation failed: {e}");
        }
    }

    // Build and send ServerHello.
    let session_id: [u8; 16] = rand::random();
    let server_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let server_hello = build_server_hello(
        proto_v,
        "openrd-server/0.0.1",
        &server_caps,
        &session_id,
        server_time,
    );
    let mut payload = Vec::new();
    ciborium::ser::into_writer(&server_hello, &mut payload)
        .context("encode ServerHello")?;

    let mut frame_buf = Vec::with_capacity(FrameHeader::SIZE + payload.len());
    Frame::encode(
        ControlFrameType::ServerHello as u8,
        &payload,
        &mut frame_buf,
    );
    send.write_all(&frame_buf).await.context("send ServerHello")?;
    info!(
        %remote,
        session_id = %hex::encode(session_id),
        "sent ServerHello"
    );

    // --- Auth exchange ----------------------------------------------------
    let auth_bytes = read_full_frame(&mut recv).await?;
    let (auth_frame, _) = Frame::parse(&auth_bytes)?;
    if auth_frame.header.frame_type != ControlFrameType::AuthRequest as u8 {
        bail!(
            "expected AuthRequest (0x03), got 0x{:02x}",
            auth_frame.header.frame_type
        );
    }
    let auth_req: Cbor = ciborium::de::from_reader(auth_frame.payload)
        .context("decode AuthRequest CBOR")?;
    let (method, credential) = parse_auth_request(&auth_req)?;
    info!(%remote, %method, cred_len = credential.len(), "received AuthRequest");

    let (status, permission, identity) = match method.as_str() {
        "pin" => {
            if constant_time_eq(&credential, expected_pin.as_bytes()) {
                let id = format!("pin-user-{}", &hex::encode(&session_id[..4]));
                info!(%remote, identity = %id, "PIN auth ok");
                (ErrorCode::Ok as u32, "interactive", id)
            } else {
                warn!(%remote, "PIN auth failed");
                (ErrorCode::AuthFailed as u32, "view-only", String::new())
            }
        }
        other => {
            warn!(%remote, method = other, "unsupported auth method");
            (ErrorCode::NotImplemented as u32, "view-only", String::new())
        }
    };

    let auth_result = build_auth_result(status, permission, &identity);
    let mut payload = Vec::new();
    ciborium::ser::into_writer(&auth_result, &mut payload)
        .context("encode AuthResult")?;
    let mut frame_buf = Vec::with_capacity(FrameHeader::SIZE + payload.len());
    Frame::encode(
        ControlFrameType::AuthResult as u8,
        &payload,
        &mut frame_buf,
    );
    send.write_all(&frame_buf).await.context("send AuthResult")?;
    info!(%remote, status, "sent AuthResult");

    if status != ErrorCode::Ok as u32 {
        let _ = send.finish();
        return Ok(());
    }

    // TODO: post-auth Control loop — channel open/close, pings, etc.
    // Returns here; caller closes the connection.
    let _ = send.finish();
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn parse_auth_request(v: &Cbor) -> Result<(String, Vec<u8>)> {
    let map = v.as_map().ok_or_else(|| anyhow!("AuthRequest not a map"))?;
    let mut method: Option<String> = None;
    let mut credential: Option<Vec<u8>> = None;
    for (k, val) in map {
        let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
            Some(k) => k,
            None => continue,
        };
        match key {
            1 => method = val.as_text().map(|s| s.to_owned()),
            2 => credential = val.as_bytes().map(|b| b.to_vec()),
            _ => {}
        }
    }
    let method = method.ok_or_else(|| anyhow!("AuthRequest missing key 1 (method)"))?;
    let credential =
        credential.ok_or_else(|| anyhow!("AuthRequest missing key 2 (credential)"))?;
    Ok((method, credential))
}

fn build_auth_result(status: u32, permission: &str, identity: &str) -> Cbor {
    Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer((status as u64).into())),
        (Cbor::Integer(2u32.into()), Cbor::Text(permission.to_owned())),
        (Cbor::Integer(3u32.into()), Cbor::Text(identity.to_owned())),
    ])
}

/// Read exactly one complete frame (header + payload) from `recv`.
async fn read_full_frame(recv: &mut RecvStream) -> Result<Vec<u8>> {
    let mut header_buf = [0u8; FrameHeader::SIZE];
    recv.read_exact(&mut header_buf)
        .await
        .map_err(|e| anyhow!("read frame header: {e}"))?;
    let header = FrameHeader::parse(&header_buf)?;

    if (header.length as usize) > MAX_FRAME_LENGTH {
        bail!("frame too large: {} bytes", header.length);
    }

    let mut full = vec![0u8; FrameHeader::SIZE + header.length as usize];
    full[..FrameHeader::SIZE].copy_from_slice(&header_buf);
    recv.read_exact(&mut full[FrameHeader::SIZE..])
        .await
        .map_err(|e| anyhow!("read frame payload: {e}"))?;
    Ok(full)
}

/// Extract `(protocol_version, client_name, client_capabilities)` from
/// a parsed CBOR `ClientHello`. Unknown / extra keys are silently
/// ignored so future versions of the client can add fields without
/// breaking us. A missing capabilities map yields
/// `Capabilities::default()`.
fn parse_client_hello(v: &Cbor) -> Result<(u64, String, Capabilities)> {
    let map = v
        .as_map()
        .ok_or_else(|| anyhow!("ClientHello is not a CBOR map"))?;

    let mut proto_v: Option<u64> = None;
    let mut client_name: Option<String> = None;
    let mut caps = Capabilities::default();

    for (k, val) in map {
        let key_u64 = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
            Some(n) => n,
            None => continue,
        };
        match key_u64 {
            1 => proto_v = val.as_integer().and_then(|i| u64::try_from(i).ok()),
            2 => client_name = val.as_text().map(|s| s.to_owned()),
            3 => caps = Capabilities::from_cbor(val),
            // key 4 (session_id_hint) is for resumption; ignored in this stub.
            _ => {}
        }
    }

    let proto_v =
        proto_v.ok_or_else(|| anyhow!("ClientHello missing key 1 (protocol_version)"))?;
    let client_name = client_name.unwrap_or_else(|| "<unknown>".to_string());
    Ok((proto_v, client_name, caps))
}

fn build_server_hello(
    proto_v: u64,
    server_name: &str,
    caps: &Capabilities,
    session_id: &[u8; 16],
    server_time: u64,
) -> Cbor {
    Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(proto_v.into())),
        (Cbor::Integer(2u32.into()), Cbor::Text(server_name.to_owned())),
        (Cbor::Integer(3u32.into()), caps.to_cbor()),
        (Cbor::Integer(4u32.into()), Cbor::Bytes(session_id.to_vec())),
        (Cbor::Integer(5u32.into()), Cbor::Integer(server_time.into())),
    ])
}
