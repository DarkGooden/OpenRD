//! Control channel handler.
//!
//! After the hello + PIN auth, this dispatches per-channel handlers
//! over the Control bidirectional stream:
//!
//! - **Server-initiated:** the server sends `OpenChannel` on Control
//!   then opens a unidirectional stream and emits the channel's frames.
//!   Currently: Display (synthetic test pattern), Cursor (synthetic
//!   moves).
//! - **Client-initiated:** the client sends `OpenChannel`; the server
//!   sends `OpenChannelAck(OK)` then synchronously accepts the next
//!   uni/bi stream and spawns a handler. Currently: Input (uni),
//!   Clipboard (bi), Chat (bi), File (bi).
//!
//! The loop exits when the client closes the Control stream.

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use ciborium::Value as Cbor;
use openrd_proto::clipboard::ClipboardFrameType;
use openrd_proto::control::ControlFrameType;
use openrd_proto::cursor::CursorFrameType;
use openrd_proto::display::DisplayFrameType;
use openrd_proto::input::{InputFrameType, KeyEvent, TextInput};
use openrd_proto::{
    Capabilities, ChannelKind, ErrorCode, Frame, FrameHeader, NegotiatedProfile,
    ProtocolVersion, MAX_FRAME_LENGTH,
};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use wtransport::{Connection, RecvStream, SendStream};

pub async fn handle_control_stream(
    conn: &Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    remote: SocketAddr,
    expected_pin: &str,
) -> Result<()> {
    hello_exchange(&mut send, &mut recv, remote).await?;

    if !auth_exchange(&mut send, &mut recv, remote, expected_pin).await? {
        return Ok(());
    }

    // Announce server-initiated channels (Display, Cursor) on Control
    // and spawn their handlers. The handlers open their own uni
    // streams; the announcement tells the client what to expect.
    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    // QUIC streams aren't visible to the peer until the opener writes
    // bytes. Writing the first frame synchronously in the main task
    // commits the stream to the wire in announcement order, so the
    // client's accept_uni calls see Display then Cursor (not racing).
    send_open_channel_announce(&mut send, ChannelKind::DISPLAY.0 as u64, 100, 0).await?;
    let mut display_stream = conn.open_uni().await?.await?;
    let sp = build_stream_parameters(
        0x01,
        1920,
        1080,
        &[0x67, 0x42, 0x00, 0x1e, 0x96, 0x35, 0x40],
        &[0x68, 0xce, 0x38, 0x80],
    );
    write_frame(&mut display_stream, DisplayFrameType::StreamParameters as u8, &sp).await?;
    info!(%remote, "Display uni stream opened + StreamParameters sent");
    handles.push(tokio::spawn(async move {
        if let Err(e) = send_display_frames(display_stream, remote).await {
            warn!(%remote, "Display handler error: {e:#}");
        }
    }));

    send_open_channel_announce(&mut send, ChannelKind::CURSOR.0 as u64, 101, 0).await?;
    let mut cursor_stream = conn.open_uni().await?.await?;
    let shape = build_cursor_shape();
    write_frame(&mut cursor_stream, CursorFrameType::CursorShape as u8, &shape).await?;
    info!(%remote, "Cursor uni stream opened + CursorShape sent");
    handles.push(tokio::spawn(async move {
        if let Err(e) = send_cursor_moves(cursor_stream, remote).await {
            warn!(%remote, "Cursor handler error: {e:#}");
        }
    }));

    // Client-initiated channel dispatcher loop.
    loop {
        let frame_bytes = match read_full_frame_opt(&mut recv).await? {
            Some(b) => b,
            None => break,
        };
        let (frame, _) = Frame::parse(&frame_bytes)?;
        let ft = frame.header.frame_type;

        if ft != ControlFrameType::OpenChannel as u8 {
            warn!(%remote, "unexpected Control frame type 0x{ft:02x}; ignoring");
            continue;
        }

        let val: Cbor =
            ciborium::de::from_reader(frame.payload).context("decode OpenChannel")?;
        let (kind, channel_id, _stream_id) = parse_open_channel(&val)?;
        info!(
            %remote,
            kind = format!("0x{kind:04x}"),
            channel_id,
            "received OpenChannel"
        );

        let status = if is_supported_client_channel(kind) {
            ErrorCode::Ok as u32
        } else {
            ErrorCode::NotImplemented as u32
        };
        send_open_channel_ack(&mut send, channel_id, status).await?;
        info!(%remote, channel_id, status, "sent OpenChannelAck");

        if status != ErrorCode::Ok as u32 {
            continue;
        }

        // Synchronously accept the corresponding stream so subsequent
        // OpenChannels don't race for the wrong stream, then spawn the
        // handler.
        match kind {
            x if x == ChannelKind::INPUT.0 as u64 => {
                let recv_stream = conn.accept_uni().await?;
                info!(%remote, "accepted Input uni stream");
                handles.push(tokio::spawn(async move {
                    if let Err(e) = handle_input_stream(recv_stream, remote).await {
                        warn!(%remote, "Input handler error: {e:#}");
                    }
                }));
            }
            x if x == ChannelKind::CLIPBOARD.0 as u64 => {
                let (send_s, recv_s) = conn.accept_bi().await?;
                info!(%remote, "accepted Clipboard bidi stream");
                handles.push(tokio::spawn(async move {
                    if let Err(e) = handle_clipboard_stream(send_s, recv_s, remote).await {
                        warn!(%remote, "Clipboard handler error: {e:#}");
                    }
                }));
            }
            _ => unreachable!("filtered above"),
        }
    }

    info!(%remote, "Control stream ended; waiting for channel handlers");
    for h in handles {
        let _ = h.await;
    }
    Ok(())
}

fn is_supported_client_channel(kind: u64) -> bool {
    matches!(
        kind,
        x if x == ChannelKind::INPUT.0 as u64
            || x == ChannelKind::CLIPBOARD.0 as u64
    )
}

// ===== Hello exchange =================================================

async fn hello_exchange(
    send: &mut SendStream,
    recv: &mut RecvStream,
    remote: SocketAddr,
) -> Result<()> {
    let frame_bytes = read_full_frame(recv).await?;
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

    let hello: Cbor =
        ciborium::de::from_reader(frame.payload).context("decode ClientHello CBOR")?;
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

    let session_id: [u8; 16] = rand::random();
    let server_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let server_hello =
        build_server_hello(proto_v, "openrd-server/0.0.1", &server_caps, &session_id, server_time);
    let mut payload = Vec::new();
    ciborium::ser::into_writer(&server_hello, &mut payload).context("encode ServerHello")?;
    write_frame(send, ControlFrameType::ServerHello as u8, &payload).await?;
    info!(
        %remote,
        session_id = %hex::encode(session_id),
        "sent ServerHello"
    );
    Ok(())
}

// ===== Auth ===========================================================

async fn auth_exchange(
    send: &mut SendStream,
    recv: &mut RecvStream,
    remote: SocketAddr,
    expected_pin: &str,
) -> Result<bool> {
    let auth_bytes = read_full_frame(recv).await?;
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
                let mut id_seed = [0u8; 4];
                id_seed.copy_from_slice(&rand::random::<[u8; 4]>());
                let id = format!("pin-user-{}", hex::encode(id_seed));
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
    ciborium::ser::into_writer(&auth_result, &mut payload).context("encode AuthResult")?;
    write_frame(send, ControlFrameType::AuthResult as u8, &payload).await?;
    info!(%remote, status, "sent AuthResult");

    Ok(status == ErrorCode::Ok as u32)
}

// ===== Input channel ==================================================

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
            other => info!(%remote, ?other, len = frame.payload.len(), "Input::(unhandled)"),
        }
        count += 1;
    }
    info!(%remote, count, "Input stream closed");
    Ok(())
}

// ===== Clipboard channel (bidirectional) ==============================

async fn handle_clipboard_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    remote: SocketAddr,
) -> Result<()> {
    // Server's clipboard fixture: the in-memory clipboard contents the
    // server will return when the client requests them. Mirrors what a
    // real implementation would read from the desktop session.
    let server_clipboard_text = "server clipboard: olá from the OpenRD server";

    loop {
        let frame_bytes = match read_full_frame_opt(&mut recv).await? {
            Some(b) => b,
            None => break,
        };
        let (frame, _) = Frame::parse(&frame_bytes)?;
        let kind = ClipboardFrameType::from_u8(frame.header.frame_type)?;
        match kind {
            ClipboardFrameType::OfferTypes => {
                let val: Cbor = ciborium::de::from_reader(frame.payload)
                    .context("decode OfferTypes")?;
                let types: Vec<String> = val
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_text().map(|s| s.to_owned()))
                            .collect()
                    })
                    .unwrap_or_default();
                info!(%remote, ?types, "Clipboard::OfferTypes (client offers)");
            }
            ClipboardFrameType::RequestContent => {
                let val: Cbor = ciborium::de::from_reader(frame.payload)
                    .context("decode RequestContent")?;
                let (request_id, mime) = parse_request_content(&val);
                info!(%remote, request_id, %mime, "Clipboard::RequestContent");

                // Respond with the server's clipboard (text/plain only
                // in this scaffold).
                let body = if mime.starts_with("text/plain") {
                    server_clipboard_text.as_bytes().to_vec()
                } else {
                    Vec::new()
                };
                let content = build_clipboard_content(request_id, &mime, &body);
                let mut payload = Vec::new();
                ciborium::ser::into_writer(&content, &mut payload)?;
                write_frame(&mut send, ClipboardFrameType::Content as u8, &payload).await?;
                info!(%remote, request_id, body_len = body.len(), "Clipboard::Content (sent)");
            }
            ClipboardFrameType::Content => {
                let val: Cbor = ciborium::de::from_reader(frame.payload)
                    .context("decode Content")?;
                let (request_id, mime, bytes) = parse_clipboard_content(&val);
                let preview = String::from_utf8_lossy(&bytes);
                let preview = if preview.len() > 80 {
                    format!("{}...", &preview[..80])
                } else {
                    preview.into_owned()
                };
                info!(
                    %remote,
                    request_id,
                    %mime,
                    len = bytes.len(),
                    %preview,
                    "Clipboard::Content (received from client)"
                );
            }
            other => info!(%remote, ?other, "Clipboard::(unhandled)"),
        }
    }
    info!(%remote, "Clipboard stream closed");
    let _ = send.finish().await;
    Ok(())
}

// ===== Display test pattern (server-initiated) ========================

/// Emit the post-StreamParameters frame loop on the Display stream.
/// (StreamParameters is sent synchronously in the main task so the
/// stream is committed to the wire in the right order.)
async fn send_display_frames(mut stream: SendStream, remote: SocketAddr) -> Result<()> {
    const FRAME_COUNT: u32 = 5;
    const N_SLICES: u8 = 4;
    const SLICE_PAYLOAD_BYTES: usize = 256;

    for frame_id in 0..FRAME_COUNT {
        let timestamp_us = (frame_id as u64) * 33_333;
        let is_idr = frame_id == 0;
        let flags: u8 = if is_idr { 0x01 } else { 0x00 };

        let mut header = Vec::with_capacity(15);
        header.extend(&frame_id.to_le_bytes());
        header.push(flags);
        header.extend(&timestamp_us.to_le_bytes());
        header.push(N_SLICES);
        header.push(0);
        write_frame_owned(&mut stream, DisplayFrameType::FrameHeader as u8, &header).await?;

        for slice_idx in 0..N_SLICES {
            let nal = vec![0xFFu8 ^ slice_idx; SLICE_PAYLOAD_BYTES];
            let mut slice = Vec::with_capacity(10 + nal.len());
            slice.extend(&frame_id.to_le_bytes());
            slice.push(slice_idx);
            slice.push(N_SLICES);
            slice.extend(&(nal.len() as u32).to_le_bytes());
            slice.extend(&nal);
            write_frame_owned(&mut stream, DisplayFrameType::FrameSlice as u8, &slice).await?;
        }

        let fe = frame_id.to_le_bytes().to_vec();
        write_frame_owned(&mut stream, DisplayFrameType::FrameEnd as u8, &fe).await?;

        info!(
            %remote,
            frame_id,
            idr = is_idr,
            n_slices = N_SLICES,
            timestamp_us,
            "sent Display frame"
        );
        tokio::time::sleep(Duration::from_millis(33)).await;
    }

    stream.finish().await.ok();
    info!(%remote, frames = FRAME_COUNT, "finished Display uni stream");
    Ok(())
}

// ===== Cursor test pattern (server-initiated) =========================

fn build_cursor_shape() -> Vec<u8> {
    let mut shape = Vec::with_capacity(11 + 4 * 4 * 4);
    shape.extend(&4u16.to_le_bytes());
    shape.extend(&4u16.to_le_bytes());
    shape.extend(&0u16.to_le_bytes());
    shape.extend(&0u16.to_le_bytes());
    shape.push(0x01); // format = BGRA premultiplied
    let pixels: Vec<u8> = (0..16).flat_map(|_| [0xFFu8, 0x00, 0x00, 0xFF]).collect();
    shape.extend(&(pixels.len() as u32).to_le_bytes());
    shape.extend(&pixels);
    shape
}

/// Emit the post-CursorShape move sequence on the Cursor stream.
async fn send_cursor_moves(mut stream: SendStream, remote: SocketAddr) -> Result<()> {
    // Send a few CursorMove updates.
    let moves: [(i32, i32); 5] =
        [(100, 100), (200, 150), (300, 220), (450, 360), (640, 480)];
    for (i, (x, y)) in moves.iter().enumerate() {
        let mut buf = Vec::with_capacity(8);
        buf.extend(&x.to_le_bytes());
        buf.extend(&y.to_le_bytes());
        write_frame_owned(&mut stream, CursorFrameType::CursorMove as u8, &buf).await?;
        info!(%remote, x, y, seq = i, "sent Cursor::CursorMove");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    stream.finish().await.ok();
    info!(%remote, "finished Cursor uni stream");
    Ok(())
}

// ===== CBOR helpers ===================================================

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

async fn send_open_channel_announce(
    send: &mut SendStream,
    kind: u64,
    channel_id: u64,
    stream_id: u64,
) -> Result<()> {
    let value = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(kind.into())),
        (Cbor::Integer(2u32.into()), Cbor::Integer(channel_id.into())),
        (Cbor::Integer(3u32.into()), Cbor::Integer(stream_id.into())),
        (Cbor::Integer(4u32.into()), Cbor::Map(vec![])),
    ]);
    let mut payload = Vec::new();
    ciborium::ser::into_writer(&value, &mut payload)?;
    write_frame(send, ControlFrameType::OpenChannel as u8, &payload).await
}

async fn send_open_channel_ack(
    send: &mut SendStream,
    channel_id: u64,
    status: u32,
) -> Result<()> {
    let value = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(channel_id.into())),
        (Cbor::Integer(2u32.into()), Cbor::Integer((status as u64).into())),
        (Cbor::Integer(3u32.into()), Cbor::Map(vec![])),
    ]);
    let mut payload = Vec::new();
    ciborium::ser::into_writer(&value, &mut payload)?;
    write_frame(send, ControlFrameType::OpenChannelAck as u8, &payload).await
}

fn parse_request_content(v: &Cbor) -> (u64, String) {
    let mut request_id: u64 = 0;
    let mut mime = String::new();
    if let Some(map) = v.as_map() {
        for (k, val) in map {
            let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
                Some(k) => k,
                None => continue,
            };
            match key {
                1 => {
                    if let Some(s) = val.as_text() {
                        mime = s.to_owned();
                    }
                }
                2 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        request_id = n;
                    }
                }
                _ => {}
            }
        }
    }
    (request_id, mime)
}

fn build_clipboard_content(request_id: u64, mime: &str, bytes: &[u8]) -> Cbor {
    Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(request_id.into())),
        (Cbor::Integer(2u32.into()), Cbor::Text(mime.to_owned())),
        (Cbor::Integer(3u32.into()), Cbor::Bytes(bytes.to_vec())),
        (Cbor::Integer(4u32.into()), Cbor::Bool(false)),
    ])
}

fn parse_clipboard_content(v: &Cbor) -> (u64, String, Vec<u8>) {
    let mut request_id: u64 = 0;
    let mut mime = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    if let Some(map) = v.as_map() {
        for (k, val) in map {
            let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
                Some(k) => k,
                None => continue,
            };
            match key {
                1 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        request_id = n;
                    }
                }
                2 => {
                    if let Some(s) = val.as_text() {
                        mime = s.to_owned();
                    }
                }
                3 => {
                    if let Some(b) = val.as_bytes() {
                        bytes = b.to_vec();
                    }
                }
                _ => {}
            }
        }
    }
    (request_id, mime, bytes)
}

fn build_stream_parameters(codec: u8, width: u16, height: u16, sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + 2 + sps.len() + 2 + pps.len());
    out.push(codec);
    out.extend(&width.to_le_bytes());
    out.extend(&height.to_le_bytes());
    out.extend(&(sps.len() as u16).to_le_bytes());
    out.extend(sps);
    out.extend(&(pps.len() as u16).to_le_bytes());
    out.extend(pps);
    out
}

// ===== Wire I/O helpers ===============================================

async fn write_frame(send: &mut SendStream, frame_type: u8, payload: &[u8]) -> Result<()> {
    let mut buf = Vec::with_capacity(FrameHeader::SIZE + payload.len());
    Frame::encode(frame_type, payload, &mut buf);
    send.write_all(&buf).await.map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

async fn write_frame_owned(send: &mut SendStream, frame_type: u8, payload: &[u8]) -> Result<()> {
    write_frame(send, frame_type, payload).await
}

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
