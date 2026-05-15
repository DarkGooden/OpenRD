//! Native test client for the OpenRD reference server.
//!
//! Walks through the v0 scaffolding flow: hello + auth, then exercises
//! every supported channel — server-initiated (Display, Cursor) and
//! client-initiated (Input, Clipboard). Each channel is exercised with
//! a small canned sequence; on the server side the smoke test verifies
//! the corresponding events appear in the log.

use anyhow::{bail, Context, Result};
use ciborium::Value as Cbor;
use openrd_proto::chat::ChatFrameType;
use openrd_proto::clipboard::ClipboardFrameType;
use openrd_proto::control::ControlFrameType;
use openrd_proto::cursor::CursorFrameType;
use openrd_proto::display::DisplayFrameType;
use openrd_proto::file::FileFrameType;
use openrd_proto::input::{InputFrameType, KeyEvent, Modifiers, TextInput};
use openrd_proto::{
    Capabilities, ChannelKind, ErrorCode, Frame, FrameHeader, NegotiatedProfile,
    MAX_FRAME_LENGTH, PROTOCOL_VERSION,
};
use wtransport::{ClientConfig, Connection, Endpoint, RecvStream, SendStream};

const SERVER_URL: &str = "https://127.0.0.1:4443/openrd";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = ClientConfig::builder()
        .with_bind_default()
        .with_no_cert_validation()
        .build();
    let endpoint = Endpoint::client(config)?;

    println!("connecting to {SERVER_URL}...");
    let conn = endpoint.connect(SERVER_URL).await.context("WT connect")?;
    println!("connected; opening Control bidirectional stream");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?.await?;

    let client_caps = Capabilities::default();

    // === ClientHello / ServerHello ===
    let payload = encode_client_hello(&client_caps);
    write_frame(&mut send, ControlFrameType::ClientHello as u8, &payload).await?;
    println!("sent ClientHello ({} payload bytes)", payload.len());

    let bytes = read_frame(&mut recv).await?;
    let (parsed, _) = Frame::parse(&bytes)?;
    if parsed.header.frame_type != ControlFrameType::ServerHello as u8 {
        bail!(
            "expected ServerHello (0x02), got 0x{:02x}",
            parsed.header.frame_type
        );
    }
    let value: Cbor = ciborium::de::from_reader(parsed.payload)
        .context("decode ServerHello CBOR")?;
    let server_caps = describe_server_hello(&value);

    let profile = NegotiatedProfile::negotiate(&client_caps, &server_caps)
        .context("capability negotiation failed")?;
    println!(
        "Negotiated: codec={} {}x{}@{}fps auth={:?} chat={}",
        profile.display_codec,
        profile.display_resolution.0,
        profile.display_resolution.1,
        profile.display_max_fps,
        profile.auth_methods,
        profile.chat_enabled
    );

    // === AuthRequest / AuthResult ===
    let pin = std::env::var("OPENRD_PIN").unwrap_or_else(|_| {
        eprintln!("warning: OPENRD_PIN not set; sending empty PIN");
        String::new()
    });
    let auth_payload = encode_auth_request("pin", pin.as_bytes());
    write_frame(&mut send, ControlFrameType::AuthRequest as u8, &auth_payload).await?;
    println!("\nsent AuthRequest (method=pin, credential={} B)", pin.len());

    let bytes = read_frame(&mut recv).await?;
    let (parsed, _) = Frame::parse(&bytes)?;
    if parsed.header.frame_type != ControlFrameType::AuthResult as u8 {
        bail!(
            "expected AuthResult (0x05), got 0x{:02x}",
            parsed.header.frame_type
        );
    }
    let auth_result: Cbor = ciborium::de::from_reader(parsed.payload)?;
    let (status, permission, identity) = parse_auth_result(&auth_result);
    println!(
        "AuthResult: status={status} ({:?}) permission={permission} identity={identity}",
        ErrorCode::from_u16(status as u16)
    );
    if status != 0 {
        bail!("auth failed");
    }

    // === Read server's announcements for server-initiated channels ===
    // The server sends OpenChannel(Display, ...) and OpenChannel(Cursor, ...)
    // on the Control stream right after auth.
    let mut server_uni_kinds: Vec<u64> = Vec::new();
    for _ in 0..2 {
        let bytes = read_frame(&mut recv).await?;
        let (parsed, _) = Frame::parse(&bytes)?;
        if parsed.header.frame_type != ControlFrameType::OpenChannel as u8 {
            bail!(
                "expected OpenChannel (0x06), got 0x{:02x}",
                parsed.header.frame_type
            );
        }
        let v: Cbor = ciborium::de::from_reader(parsed.payload)?;
        let (kind, channel_id, _stream_id) = parse_open_channel(&v);
        let name = match kind {
            x if x == ChannelKind::DISPLAY.0 as u64 => "Display",
            x if x == ChannelKind::CURSOR.0 as u64 => "Cursor",
            _ => "(other)",
        };
        println!(
            "server announced OpenChannel: kind=0x{kind:04x} ({name}) channel_id={channel_id}"
        );
        server_uni_kinds.push(kind);
    }

    // === Accept the server-initiated uni streams in announcement order ===
    let display_stream = conn.accept_uni().await?;
    println!("accepted Display uni stream");
    let cursor_stream = conn.accept_uni().await?;
    println!("accepted Cursor uni stream");
    let display_handle = tokio::spawn(receive_display(display_stream));
    let cursor_handle = tokio::spawn(receive_cursor(cursor_stream));

    // === Client-initiated channels in sequence ===
    open_channel_and_exercise_input(&conn, &mut send, &mut recv).await?;
    open_channel_and_exercise_clipboard(&conn, &mut send, &mut recv).await?;
    open_channel_and_exercise_chat(&conn, &mut send, &mut recv).await?;
    open_channel_and_exercise_file(&conn, &mut send, &mut recv).await?;

    // Close Control so the server's dispatcher loop ends.
    send.finish().await.ok();
    println!("finished Control stream");

    // Wait for the server-initiated receivers to drain.
    display_handle.await??;
    cursor_handle.await??;

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    conn.close(0u32.into(), b"bye");
    Ok(())
}

// ===== Input channel ===================================================

async fn open_channel_and_exercise_input(
    conn: &Connection,
    send: &mut SendStream,
    recv: &mut RecvStream,
) -> Result<()> {
    let oc = encode_open_channel(ChannelKind::INPUT.0 as u64, 1, 2);
    write_frame(send, ControlFrameType::OpenChannel as u8, &oc).await?;
    println!("\nsent OpenChannel(Input, channel_id=1)");

    expect_ack(recv, "Input").await?;

    let mut input_stream = conn.open_uni().await?.await?;
    println!("opened Input uni stream");

    let key_a_down = KeyEvent {
        keysym: 0x0061,
        scancode: 0x001E,
        modifiers: Modifiers::default(),
        flags: 0x01,
    };
    write_input_keyevent(&mut input_stream, &key_a_down).await?;
    println!("sent KeyEvent ('a' down)");

    let mut key_a_up = key_a_down;
    key_a_up.flags = 0x00;
    write_input_keyevent(&mut input_stream, &key_a_up).await?;
    println!("sent KeyEvent ('a' up)");

    write_input_textinput(
        &mut input_stream,
        &TextInput {
            text: "olá, mundo 🌍".to_owned(),
        },
    )
    .await?;
    println!("sent TextInput (\"olá, mundo 🌍\")");

    input_stream.finish().await.ok();
    println!("finished Input uni stream");
    Ok(())
}

async fn write_input_keyevent(stream: &mut SendStream, ev: &KeyEvent) -> Result<()> {
    let mut payload = Vec::new();
    ev.write_to(&mut payload);
    write_frame(stream, InputFrameType::KeyEvent as u8, &payload).await
}

async fn write_input_textinput(stream: &mut SendStream, ev: &TextInput) -> Result<()> {
    let mut payload = Vec::new();
    ev.write_to(&mut payload);
    write_frame(stream, InputFrameType::TextInput as u8, &payload).await
}

// ===== Clipboard channel ===============================================

async fn open_channel_and_exercise_clipboard(
    conn: &Connection,
    send: &mut SendStream,
    recv: &mut RecvStream,
) -> Result<()> {
    let oc = encode_open_channel(ChannelKind::CLIPBOARD.0 as u64, 2, 6);
    write_frame(send, ControlFrameType::OpenChannel as u8, &oc).await?;
    println!("\nsent OpenChannel(Clipboard, channel_id=2)");

    expect_ack(recv, "Clipboard").await?;

    let (mut clip_send, mut clip_recv) = conn.open_bi().await?.await?;
    println!("opened Clipboard bidi stream");

    // 1. Client announces what types it can offer (mirrors the spec's
    //    OfferTypes; doesn't push content).
    let offer = Cbor::Array(vec![Cbor::Text(
        "text/plain;charset=utf-8".to_owned(),
    )]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&offer, &mut buf)?;
    write_frame(&mut clip_send, ClipboardFrameType::OfferTypes as u8, &buf).await?;
    println!("sent Clipboard::OfferTypes (text/plain)");

    // 2. Client requests the server's clipboard.
    let req = Cbor::Map(vec![
        (
            Cbor::Integer(1u32.into()),
            Cbor::Text("text/plain;charset=utf-8".to_owned()),
        ),
        (Cbor::Integer(2u32.into()), Cbor::Integer(42u32.into())),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&req, &mut buf)?;
    write_frame(
        &mut clip_send,
        ClipboardFrameType::RequestContent as u8,
        &buf,
    )
    .await?;
    println!("sent Clipboard::RequestContent (request_id=42, text/plain)");

    // 3. Receive server's clipboard content.
    let content_bytes = read_frame(&mut clip_recv).await?;
    let (frame, _) = Frame::parse(&content_bytes)?;
    if frame.header.frame_type != ClipboardFrameType::Content as u8 {
        bail!(
            "expected Clipboard::Content (0x03), got 0x{:02x}",
            frame.header.frame_type
        );
    }
    let v: Cbor = ciborium::de::from_reader(frame.payload)?;
    let (request_id, mime, bytes) = parse_clipboard_content(&v);
    println!(
        "recv Clipboard::Content request_id={request_id} mime={mime} bytes=\"{}\"",
        String::from_utf8_lossy(&bytes)
    );

    // 4. Push the client's own clipboard back (server will log it).
    let content_to_send = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(100u32.into())),
        (
            Cbor::Integer(2u32.into()),
            Cbor::Text("text/plain;charset=utf-8".to_owned()),
        ),
        (
            Cbor::Integer(3u32.into()),
            Cbor::Bytes(
                "client clipboard: hello back from the test client"
                    .as_bytes()
                    .to_vec(),
            ),
        ),
        (Cbor::Integer(4u32.into()), Cbor::Bool(false)),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&content_to_send, &mut buf)?;
    write_frame(&mut clip_send, ClipboardFrameType::Content as u8, &buf).await?;
    println!("sent Clipboard::Content (back to server)");

    clip_send.finish().await.ok();
    println!("finished Clipboard bidi stream");
    Ok(())
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

// ===== Chat channel ===================================================

async fn open_channel_and_exercise_chat(
    conn: &Connection,
    send: &mut SendStream,
    recv: &mut RecvStream,
) -> Result<()> {
    let oc = encode_open_channel(ChannelKind::CHAT.0 as u64, 3, 10);
    write_frame(send, ControlFrameType::OpenChannel as u8, &oc).await?;
    println!("\nsent OpenChannel(Chat, channel_id=3)");

    expect_ack(recv, "Chat").await?;

    let (mut chat_send, mut chat_recv) = conn.open_bi().await?.await?;
    println!("opened Chat bidi stream");

    // TypingIndicator: start
    let typing = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Text("openrd-test-client".to_owned())),
        (Cbor::Integer(2u32.into()), Cbor::Text("start".to_owned())),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&typing, &mut buf)?;
    write_frame(&mut chat_send, ChatFrameType::TypingIndicator as u8, &buf).await?;
    println!("sent Chat::TypingIndicator (start)");

    // ChatMessage
    let body = "olá from the test client! how are things server-side?";
    let msg = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(7u32.into())),
        (Cbor::Integer(2u32.into()), Cbor::Text("openrd-test-client".to_owned())),
        (Cbor::Integer(3u32.into()), Cbor::Text(body.to_owned())),
        (Cbor::Integer(4u32.into()), Cbor::Integer(0u32.into())),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&msg, &mut buf)?;
    write_frame(&mut chat_send, ChatFrameType::ChatMessage as u8, &buf).await?;
    println!("sent Chat::ChatMessage (\"{body}\")");

    // Read server's echo.
    let bytes = read_frame(&mut chat_recv).await?;
    let (frame, _) = Frame::parse(&bytes)?;
    if frame.header.frame_type != ChatFrameType::ChatMessage as u8 {
        bail!(
            "expected Chat::ChatMessage echo (0x01), got 0x{:02x}",
            frame.header.frame_type
        );
    }
    let v: Cbor = ciborium::de::from_reader(frame.payload)?;
    let (msg_id, sender, echo_body) = parse_chat_message_client(&v);
    println!("recv Chat::ChatMessage echo msg_id={msg_id} sender={sender} body=\"{echo_body}\"");

    // TypingIndicator: stop
    let typing = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Text("openrd-test-client".to_owned())),
        (Cbor::Integer(2u32.into()), Cbor::Text("stop".to_owned())),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&typing, &mut buf)?;
    write_frame(&mut chat_send, ChatFrameType::TypingIndicator as u8, &buf).await?;
    println!("sent Chat::TypingIndicator (stop)");

    chat_send.finish().await.ok();
    println!("finished Chat bidi stream");
    Ok(())
}

fn parse_chat_message_client(v: &Cbor) -> (u64, String, String) {
    let mut msg_id: u64 = 0;
    let mut sender = String::new();
    let mut body = String::new();
    if let Some(map) = v.as_map() {
        for (k, val) in map {
            let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
                Some(k) => k,
                None => continue,
            };
            match key {
                1 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        msg_id = n;
                    }
                }
                2 => {
                    if let Some(s) = val.as_text() {
                        sender = s.to_owned();
                    }
                }
                3 => {
                    if let Some(s) = val.as_text() {
                        body = s.to_owned();
                    }
                }
                _ => {}
            }
        }
    }
    (msg_id, sender, body)
}

// ===== File channel ===================================================

async fn open_channel_and_exercise_file(
    conn: &Connection,
    send: &mut SendStream,
    recv: &mut RecvStream,
) -> Result<()> {
    let oc = encode_open_channel(ChannelKind::FILE.0 as u64, 4, 14);
    write_frame(send, ControlFrameType::OpenChannel as u8, &oc).await?;
    println!("\nsent OpenChannel(File, channel_id=4)");

    expect_ack(recv, "File").await?;

    let (mut file_send, mut file_recv) = conn.open_bi().await?.await?;
    println!("opened File bidi stream");

    let payload_bytes = b"Hello, world from the test client's file transfer!";
    let transfer_id: u32 = 1;
    let file_idx: u32 = 0;
    let chunk_idx: u32 = 0;

    // Manifest as a single combined frame (StartTransfer/Manifest are
    // collapsed for the scaffold). Chunk SHA-256 is a placeholder
    // (32 zero bytes) — the wire format carries it; v0 server logs
    // but doesn't verify.
    let dummy_hash = vec![0u8; 32];
    let manifest = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer((transfer_id as u64).into())),
        (Cbor::Integer(2u32.into()), Cbor::Text("upload".to_owned())),
        (Cbor::Integer(3u32.into()), Cbor::Text("/tmp/uploaded".to_owned())),
        (
            Cbor::Integer(4u32.into()),
            Cbor::Array(vec![Cbor::Map(vec![
                (Cbor::Integer(1u32.into()), Cbor::Text("hello.txt".to_owned())),
                (
                    Cbor::Integer(2u32.into()),
                    Cbor::Integer((payload_bytes.len() as u64).into()),
                ),
                (Cbor::Integer(3u32.into()), Cbor::Integer(0o644u32.into())),
                (Cbor::Integer(4u32.into()), Cbor::Integer(0u32.into())),
                (
                    Cbor::Integer(5u32.into()),
                    Cbor::Array(vec![Cbor::Bytes(dummy_hash.clone())]),
                ),
            ])]),
        ),
        (Cbor::Integer(5u32.into()), Cbor::Integer(262144u32.into())),
        (Cbor::Integer(6u32.into()), Cbor::Bytes(dummy_hash.clone())),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&manifest, &mut buf)?;
    write_frame(&mut file_send, FileFrameType::StartTransfer as u8, &buf).await?;
    println!("sent File::StartTransfer (manifest, 1 file, {} bytes)", payload_bytes.len());

    // One Chunk (binary layout): transfer_id u32, file_idx u32,
    // chunk_idx u32, bytes <u32>.
    let mut chunk = Vec::with_capacity(16 + payload_bytes.len());
    chunk.extend(&transfer_id.to_le_bytes());
    chunk.extend(&file_idx.to_le_bytes());
    chunk.extend(&chunk_idx.to_le_bytes());
    chunk.extend(&(payload_bytes.len() as u32).to_le_bytes());
    chunk.extend(payload_bytes);
    write_frame(&mut file_send, FileFrameType::Chunk as u8, &chunk).await?;
    println!("sent File::Chunk ({} bytes)", payload_bytes.len());

    // Read AckChunk.
    let bytes = read_frame(&mut file_recv).await?;
    let (frame, _) = Frame::parse(&bytes)?;
    if frame.header.frame_type != FileFrameType::AckChunk as u8 {
        bail!(
            "expected File::AckChunk (0x04), got 0x{:02x}",
            frame.header.frame_type
        );
    }
    if frame.payload.len() < 13 {
        bail!("AckChunk payload too short");
    }
    let ack_tid = u32::from_le_bytes([
        frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3],
    ]);
    let ack_status = frame.payload[12];
    println!(
        "recv File::AckChunk transfer_id={ack_tid} chunk_idx={chunk_idx} status={ack_status}"
    );

    // EndTransfer (CBOR with transfer_id + status).
    let end = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer((transfer_id as u64).into())),
        (Cbor::Integer(2u32.into()), Cbor::Integer(0u32.into())),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&end, &mut buf)?;
    write_frame(&mut file_send, FileFrameType::EndTransfer as u8, &buf).await?;
    println!("sent File::EndTransfer (status=0)");

    file_send.finish().await.ok();
    println!("finished File bidi stream");
    Ok(())
}

// ===== Display receiver ===============================================

async fn receive_display(mut recv: RecvStream) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let dump_path = std::env::var("OPENRD_DISPLAY_DUMP")
        .unwrap_or_else(|_| "/tmp/openrd-display.h264".to_string());
    let mut dump = tokio::fs::File::create(&dump_path)
        .await
        .with_context(|| format!("create dump file {dump_path}"))?;
    println!("dumping received H.264 NAL stream to {dump_path}");

    let mut params_count = 0u32;
    let mut header_count = 0u32;
    let mut slice_count = 0u32;
    let mut end_count = 0u32;
    let mut total_wire_bytes = 0u64;
    let mut total_nal_bytes = 0u64;

    loop {
        let frame_bytes = match read_frame_opt(&mut recv).await? {
            Some(b) => b,
            None => break,
        };
        total_wire_bytes += frame_bytes.len() as u64;
        let (frame, _) = Frame::parse(&frame_bytes)?;
        let kind = DisplayFrameType::from_u8(frame.header.frame_type)?;
        match kind {
            DisplayFrameType::StreamParameters => {
                if frame.payload.len() >= 5 {
                    let codec = frame.payload[0];
                    let width = u16::from_le_bytes([frame.payload[1], frame.payload[2]]);
                    let height = u16::from_le_bytes([frame.payload[3], frame.payload[4]]);
                    println!("Display::StreamParameters codec=0x{codec:02x} {width}x{height}");
                }
                params_count += 1;
            }
            DisplayFrameType::FrameHeader => header_count += 1,
            DisplayFrameType::FrameSlice => {
                // FrameSlice payload layout: frame_id u32 (4) + slice_idx u8 (1)
                // + total_slices u8 (1) + nal_length u32 (4) + nal bytes.
                if frame.payload.len() >= 10 {
                    let nal_len = u32::from_le_bytes([
                        frame.payload[6],
                        frame.payload[7],
                        frame.payload[8],
                        frame.payload[9],
                    ]) as usize;
                    let nal = &frame.payload[10..10 + nal_len.min(frame.payload.len() - 10)];
                    // Write Annex-B: 0x00 0x00 0x00 0x01 then the NAL bytes.
                    dump.write_all(&[0, 0, 0, 1]).await?;
                    dump.write_all(nal).await?;
                    total_nal_bytes += nal.len() as u64;
                }
                slice_count += 1;
            }
            DisplayFrameType::FrameEnd => end_count += 1,
        }
    }

    dump.flush().await?;
    drop(dump);

    println!(
        "Display summary: params={params_count} frames={header_count} slices={slice_count} ends={end_count} wire_bytes={total_wire_bytes} nal_bytes={total_nal_bytes}"
    );
    Ok(())
}

// ===== Cursor receiver ================================================

async fn receive_cursor(mut recv: RecvStream) -> Result<()> {
    let mut shape_count = 0u32;
    let mut move_count = 0u32;
    let mut hidden_count = 0u32;
    let mut total_bytes = 0u64;
    let mut last_move = (0i32, 0i32);

    loop {
        let frame_bytes = match read_frame_opt(&mut recv).await? {
            Some(b) => b,
            None => break,
        };
        total_bytes += frame_bytes.len() as u64;
        let (frame, _) = Frame::parse(&frame_bytes)?;
        let kind = CursorFrameType::from_u8(frame.header.frame_type)?;
        match kind {
            CursorFrameType::CursorShape => {
                if frame.payload.len() >= 11 {
                    let w = u16::from_le_bytes([frame.payload[0], frame.payload[1]]);
                    let h = u16::from_le_bytes([frame.payload[2], frame.payload[3]]);
                    let fmt = frame.payload[8];
                    println!("Cursor::CursorShape {w}x{h} format=0x{fmt:02x}");
                }
                shape_count += 1;
            }
            CursorFrameType::CursorMove => {
                if frame.payload.len() >= 8 {
                    let x = i32::from_le_bytes([
                        frame.payload[0],
                        frame.payload[1],
                        frame.payload[2],
                        frame.payload[3],
                    ]);
                    let y = i32::from_le_bytes([
                        frame.payload[4],
                        frame.payload[5],
                        frame.payload[6],
                        frame.payload[7],
                    ]);
                    last_move = (x, y);
                }
                move_count += 1;
            }
            CursorFrameType::CursorHidden => hidden_count += 1,
        }
    }

    println!(
        "Cursor summary: shapes={shape_count} moves={move_count} hidden={hidden_count} last_move={last_move:?} bytes={total_bytes}"
    );
    Ok(())
}

// ===== Wire helpers ===================================================

async fn write_frame(stream: &mut SendStream, frame_type: u8, payload: &[u8]) -> Result<()> {
    let mut buf = Vec::with_capacity(FrameHeader::SIZE + payload.len());
    Frame::encode(frame_type, payload, &mut buf);
    stream.write_all(&buf).await?;
    Ok(())
}

async fn read_frame(recv: &mut RecvStream) -> Result<Vec<u8>> {
    let mut header_buf = [0u8; FrameHeader::SIZE];
    read_exact_inner(recv, &mut header_buf).await?;
    let h = FrameHeader::parse(&header_buf)?;
    if (h.length as usize) > MAX_FRAME_LENGTH {
        bail!("oversized frame: {} bytes", h.length);
    }
    let mut full = vec![0u8; FrameHeader::SIZE + h.length as usize];
    full[..FrameHeader::SIZE].copy_from_slice(&header_buf);
    read_exact_inner(recv, &mut full[FrameHeader::SIZE..]).await?;
    Ok(full)
}

async fn read_frame_opt(recv: &mut RecvStream) -> Result<Option<Vec<u8>>> {
    let mut header_buf = [0u8; FrameHeader::SIZE];
    let mut filled = 0;
    while filled < FrameHeader::SIZE {
        match recv.read(&mut header_buf[filled..]).await? {
            Some(0) | None => {
                if filled == 0 {
                    return Ok(None);
                }
                bail!("stream ended mid-header ({filled}/6 bytes)");
            }
            Some(n) => filled += n,
        }
    }
    let h = FrameHeader::parse(&header_buf)?;
    if (h.length as usize) > MAX_FRAME_LENGTH {
        bail!("oversized frame: {} bytes", h.length);
    }
    let mut full = vec![0u8; FrameHeader::SIZE + h.length as usize];
    full[..FrameHeader::SIZE].copy_from_slice(&header_buf);
    read_exact_inner(recv, &mut full[FrameHeader::SIZE..]).await?;
    Ok(Some(full))
}

async fn read_exact_inner(recv: &mut RecvStream, buf: &mut [u8]) -> Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match recv.read(&mut buf[filled..]).await? {
            Some(0) | None => bail!("stream ended early ({}/{} bytes)", filled, buf.len()),
            Some(n) => filled += n,
        }
    }
    Ok(())
}

async fn expect_ack(recv: &mut RecvStream, label: &str) -> Result<()> {
    let bytes = read_frame(recv).await?;
    let (frame, _) = Frame::parse(&bytes)?;
    if frame.header.frame_type != ControlFrameType::OpenChannelAck as u8 {
        bail!(
            "expected OpenChannelAck for {label}, got 0x{:02x}",
            frame.header.frame_type
        );
    }
    let v: Cbor = ciborium::de::from_reader(frame.payload)?;
    let (channel_id, status) = parse_open_channel_ack(&v);
    println!("recv OpenChannelAck ({label}): channel_id={channel_id} status={status}");
    if status != 0 {
        bail!("server refused {label}: status {status}");
    }
    Ok(())
}

// ===== CBOR helpers ===================================================

fn encode_client_hello(caps: &Capabilities) -> Vec<u8> {
    let value = Cbor::Map(vec![
        (
            Cbor::Integer(1u32.into()),
            Cbor::Integer((PROTOCOL_VERSION.0 as u32).into()),
        ),
        (
            Cbor::Integer(2u32.into()),
            Cbor::Text("openrd-test-client/0.0.1".to_owned()),
        ),
        (Cbor::Integer(3u32.into()), caps.to_cbor()),
    ]);
    let mut out = Vec::new();
    ciborium::ser::into_writer(&value, &mut out).expect("encode ClientHello");
    out
}

fn describe_server_hello(v: &Cbor) -> Capabilities {
    println!("ServerHello fields:");
    let mut server_caps = Capabilities::default();
    let map = match v.as_map() {
        Some(m) => m,
        None => {
            println!("  (not a map)");
            return server_caps;
        }
    };
    for (k, val) in map {
        let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
            Some(k) => k,
            None => continue,
        };
        match key {
            1 => println!(
                "  protocol_version: {}",
                val.as_integer().and_then(|i| u64::try_from(i).ok()).unwrap_or(0)
            ),
            2 => println!("  server_name:      \"{}\"", val.as_text().unwrap_or("?")),
            3 => {
                server_caps = Capabilities::from_cbor(val);
                println!("  capabilities.profile:        {}", server_caps.profile);
                println!("  capabilities.auth_methods:   {:?}", server_caps.auth_methods);
                println!("  capabilities.display_codecs: {:?}", server_caps.display_codecs);
            }
            4 => println!(
                "  session_id:       {}",
                val.as_bytes().map(hex::encode).unwrap_or_else(|| "?".into())
            ),
            5 => println!(
                "  server_time:      {}",
                val.as_integer().and_then(|i| u64::try_from(i).ok()).unwrap_or(0)
            ),
            _ => {}
        }
    }
    server_caps
}

fn encode_auth_request(method: &str, credential: &[u8]) -> Vec<u8> {
    let value = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Text(method.to_owned())),
        (Cbor::Integer(2u32.into()), Cbor::Bytes(credential.to_vec())),
    ]);
    let mut out = Vec::new();
    ciborium::ser::into_writer(&value, &mut out).expect("encode AuthRequest");
    out
}

fn parse_auth_result(v: &Cbor) -> (u32, String, String) {
    let mut status: u32 = 0;
    let mut permission = String::new();
    let mut identity = String::new();
    if let Some(map) = v.as_map() {
        for (k, val) in map {
            let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
                Some(k) => k,
                None => continue,
            };
            match key {
                1 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        status = n as u32;
                    }
                }
                2 => {
                    if let Some(s) = val.as_text() {
                        permission = s.to_owned();
                    }
                }
                3 => {
                    if let Some(s) = val.as_text() {
                        identity = s.to_owned();
                    }
                }
                _ => {}
            }
        }
    }
    (status, permission, identity)
}

fn encode_open_channel(kind: u64, channel_id: u64, stream_id: u64) -> Vec<u8> {
    let value = Cbor::Map(vec![
        (Cbor::Integer(1u32.into()), Cbor::Integer(kind.into())),
        (Cbor::Integer(2u32.into()), Cbor::Integer(channel_id.into())),
        (Cbor::Integer(3u32.into()), Cbor::Integer(stream_id.into())),
        (Cbor::Integer(4u32.into()), Cbor::Map(vec![])),
    ]);
    let mut out = Vec::new();
    ciborium::ser::into_writer(&value, &mut out).expect("encode OpenChannel");
    out
}

fn parse_open_channel(v: &Cbor) -> (u64, u64, u64) {
    let mut kind: u64 = 0;
    let mut channel_id: u64 = 0;
    let mut stream_id: u64 = 0;
    if let Some(map) = v.as_map() {
        for (k, val) in map {
            let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
                Some(k) => k,
                None => continue,
            };
            match key {
                1 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        kind = n;
                    }
                }
                2 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        channel_id = n;
                    }
                }
                3 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        stream_id = n;
                    }
                }
                _ => {}
            }
        }
    }
    (kind, channel_id, stream_id)
}

fn parse_open_channel_ack(v: &Cbor) -> (u64, u32) {
    let mut channel_id: u64 = 0;
    let mut status: u32 = 0;
    if let Some(map) = v.as_map() {
        for (k, val) in map {
            let key = match k.as_integer().and_then(|i| u64::try_from(i).ok()) {
                Some(k) => k,
                None => continue,
            };
            match key {
                1 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        channel_id = n;
                    }
                }
                2 => {
                    if let Some(n) = val.as_integer().and_then(|i| u64::try_from(i).ok()) {
                        status = n as u32;
                    }
                }
                _ => {}
            }
        }
    }
    (channel_id, status)
}
