//! Control channel handler — v0 scaffolding (hello + PIN auth + one
//! Input channel exchange).

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use ciborium::Value as Cbor;
use openrd_proto::control::ControlFrameType;
use openrd_proto::input::{InputFrameType, KeyEvent, TextInput};
use openrd_proto::{
    Capabilities, ChannelKind, ErrorCode, Frame, FrameHeader, NegotiatedProfile,
    ProtocolVersion, MAX_FRAME_LENGTH,
};
use tracing::{info, warn};
use wtransport::{Connection, RecvStream, SendStream};

pub async fn handle_control_stream(
    conn: &Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    remote: SocketAddr,
    expected_pin: &str,
) -> Result<()> {
    // --- ClientHello ------------------------------------------------------
    let frame_bytes = read_full_frame(&mut recv).await?;
    let (frame, _) = Frame::parse(&frame_bytes)?;
    if frame.header.version != ProtocolVersion::V0 {
        bail!("unsupported protocol version {:?}", frame.header.version);
    }
    if frame.header.frame_type != ControlFrameType::ClientHello as u8 {
        bail!(
            "expected ClientHello (0x01), got 0x{:02x}",
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

    let server_caps = Capabilities::default();
    match NegotiatedProfile::negotiate(&client_caps, &server_caps) {
        Ok(p) => info!(
            %remote,
            display_codec = %p.display_codec,
            auth_methods = ?p.auth_methods,
            chat = p.chat_enabled,
            "negotiated profile"
        ),
        Err(e) => warn!(%remote, "capability negotiation failed: {e}"),
    }

    // --- ServerHello ------------------------------------------------------
    let session_id: [u8; 16] = rand::random();
    let server_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let server_hello =
        build_server_hello(proto_v, "openrd-server/0.0.1", &server_caps, &session_id, server_time);
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

    // --- Auth -------------------------------------------------------------
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
        return Ok(());
    }

    // --- OpenChannel(Input) ----------------------------------------------
    let oc_bytes = read_full_frame(&mut recv).await?;
    let (oc_frame, _) = Frame::parse(&oc_bytes)?;
    if oc_frame.header.frame_type != ControlFrameType::OpenChannel as u8 {
        bail!(
            "expected OpenChannel (0x06), got 0x{:02x}",
            oc_frame.header.frame_type
        );
    }
    let oc_val: Cbor =
        ciborium::de::from_reader(oc_frame.payload).context("decode OpenChannel")?;
    let (kind, channel_id, _stream_id) = parse_open_channel(&oc_val)?;
    info!(
        %remote,
        kind = format!("0x{kind:04x}"),
        channel_id,
        "received OpenChannel"
    );

    if kind != ChannelKind::INPUT.0 as u64 {
        warn!(%remote, "v0 scaffold accepts Input channel only");
        let ack = build_open_channel_ack(channel_id, ErrorCode::NotImplemented as u32);
        let mut ack_payload = Vec::new();
        ciborium::ser::into_writer(&ack, &mut ack_payload)?;
        let mut ack_frame = Vec::with_capacity(FrameHeader::SIZE + ack_payload.len());
        Frame::encode(
            ControlFrameType::OpenChannelAck as u8,
            &ack_payload,
            &mut ack_frame,
        );
        send.write_all(&ack_frame).await?;
        return Ok(());
    }

    let ack = build_open_channel_ack(channel_id, ErrorCode::Ok as u32);
    let mut ack_payload = Vec::new();
    ciborium::ser::into_writer(&ack, &mut ack_payload)?;
    let mut ack_frame = Vec::with_capacity(FrameHeader::SIZE + ack_payload.len());
    Frame::encode(
        ControlFrameType::OpenChannelAck as u8,
        &ack_payload,
        &mut ack_frame,
    );
    send.write_all(&ack_frame).await?;
    info!(%remote, channel_id, "sent OpenChannelAck(OK)");

    let input_recv = conn
        .accept_uni()
        .await
        .context("accept Input uni stream")?;
    info!(%remote, "accepted Input uni stream");
    handle_input_stream(input_recv, remote).await?;

    Ok(())
}

async fn handle_input_stream(mut recv: RecvStream, remote: SocketAddr) -> Result<()> {
    let mut count: u64 = 0;
    loop {
        let frame_bytes = match read_full_frame_opt(&mut recv).await? {
            Some(b) => b,
            None => break,
        };
        let (frame, _) = Frame::parse(&frame_bytes)?;
        let kind = InputFrameType::from_u8(frame.header.frame_type)?;
        match kind {
            InputFrameType::KeyEvent => {
                let ev = KeyEvent::parse(frame.payload)?;
                info!(
                    %remote,
                    keysym = format!("0x{:04x}", ev.keysym),
                    scancode = ev.scancode,
                    modifiers = ev.modifiers.0,
                    down = ev.down(),
                    "Input::KeyEvent"
                );
            }
            InputFrameType::TextInput => {
                let ev = TextInput::parse(frame.payload)?;
                info!(%remote, text = %ev.text, "Input::TextInput");
            }
            other => {
                info!(%remote, ?other, len = frame.payload.len(), "Input::(unhandled)");
            }
        }
        count += 1;
    }
    info!(%remote, count, "Input stream closed");
    Ok(())
}

/// Read one complete frame off `recv`. Errors if the stream ends
/// mid-frame.
async fn read_full_frame(recv: &mut RecvStream) -> Result<Vec<u8>> {
    let mut header_buf = [0u8; FrameHeader::SIZE];
    read_exact(recv, &mut header_buf)
        .await
        .context("read frame header")?;
    let h = FrameHeader::parse(&header_buf)?;
    if (h.length as usize) > MAX_FRAME_LENGTH {
        bail!("frame too large: {} bytes", h.length);
    }
    let mut full = vec![0u8; FrameHeader::SIZE + h.length as usize];
    full[..FrameHeader::SIZE].copy_from_slice(&header_buf);
    read_exact(recv, &mut full[FrameHeader::SIZE..])
        .await
        .context("read frame payload")?;
    Ok(full)
}

/// Variant of `read_full_frame` that returns `Ok(None)` on a clean
/// stream FIN before any bytes of the next frame.
async fn read_full_frame_opt(recv: &mut RecvStream) -> Result<Option<Vec<u8>>> {
    let mut header_buf = [0u8; FrameHeader::SIZE];
    let mut filled = 0;
    while filled < FrameHeader::SIZE {
        match recv.read(&mut header_buf[filled..]).await? {
            Some(0) | None => {
                if filled == 0 {
                    return Ok(None);
                }
                bail!("stream ended mid-header ({filled} of 6 bytes)");
            }
            Some(n) => filled += n,
        }
    }
    let h = FrameHeader::parse(&header_buf)?;
    if (h.length as usize) > MAX_FRAME_LENGTH {
        bail!("frame too large: {} bytes", h.length);
    }
    let mut full = vec![0u8; FrameHeader::SIZE + h.length as usize];
    full[..FrameHeader::SIZE].copy_from_slice(&header_buf);
    read_exact(recv, &mut full[FrameHeader::SIZE..]).await?;
    Ok(Some(full))
}

/// Fill `buf` from `recv`. wtransport's stream API is one byte / chunk
/// at a time; abstract here so the rest of the file looks like quinn.
async fn read_exact(recv: &mut RecvStream, buf: &mut [u8]) -> Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match recv.read(&mut buf[filled..]).await? {
            Some(0) | None => bail!("stream ended early ({} of {})", filled, buf.len()),
            Some(n) => filled += n,
        }
    }
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

fn parse_open_channel(v: &Cbor) -> Result<(u64, u64, u64)> {
    let map = v.as_map().ok_or_else(|| anyhow!("OpenChannel not a map"))?;
    let mut kind: Option<u64> = None;
    let mut channel_id: Option<u64> = None;
    let mut stream_id: u64 = 0;
    for (k, val) in map {
        let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
            Some(k) => k,
            None => continue,
        };
        match key {
            1 => kind = val.as_integer().and_then(|i| u64::try_from(i).ok()),
            2 => channel_id = val.as_integer().and_then(|i| u64::try_from(i).ok()),
            3 => {
                if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                    stream_id = n;
                }
            }
            _ => {}
        }
    }
    let kind = kind.ok_or_else(|| anyhow!("OpenChannel missing key 1 (kind)"))?;
    let channel_id =
        channel_id.ok_or_else(|| anyhow!("OpenChannel missing key 2 (channel_id)"))?;
    Ok((kind, channel_id, stream_id))
}

fn build_open_channel_ack(channel_id: u64, status: u32) -> Cbor {
    Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(channel_id.into())),
        (Cbor::Integer(2u32.into()), Cbor::Integer((status as u64).into())),
        (Cbor::Integer(3u32.into()), Cbor::Map(vec![])),
    ])
}
