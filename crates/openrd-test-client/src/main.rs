//! Native test client for the OpenRD reference server.
//!
//! Connects via WebTransport, opens the Control bidi stream, runs the
//! v0 scaffolding flow: ClientHello / ServerHello / AuthRequest /
//! AuthResult / OpenChannel(Input) / OpenChannelAck / a few input
//! events on a unidirectional stream / clean close.
//!
//! Skips server-certificate validation; dev only.

use anyhow::{bail, Context, Result};
use ciborium::Value as Cbor;
use openrd_proto::control::ControlFrameType;
use openrd_proto::display::DisplayFrameType;
use openrd_proto::input::{InputFrameType, KeyEvent, Modifiers, TextInput};
use openrd_proto::{
    Capabilities, ChannelKind, ErrorCode, Frame, FrameHeader, NegotiatedProfile,
    MAX_FRAME_LENGTH, PROTOCOL_VERSION,
};
use wtransport::{ClientConfig, Endpoint};

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

    let mut bi = conn.open_bi().await.context("open_bi")?.await?;
    let send = &mut bi.0;
    let recv = &mut bi.1;

    let client_caps = Capabilities::default();

    // --- ClientHello ------------------------------------------------------
    let payload = encode_client_hello(&client_caps);
    let mut frame = Vec::with_capacity(FrameHeader::SIZE + payload.len());
    Frame::encode(
        ControlFrameType::ClientHello as u8,
        &payload,
        &mut frame,
    );
    send.write_all(&frame).await?;
    println!(
        "sent ClientHello (frame {} B, payload {} B)",
        frame.len(),
        payload.len()
    );

    // --- ServerHello ------------------------------------------------------
    let bytes = read_frame(recv).await?;
    let (parsed, _) = Frame::parse(&bytes)?;
    println!(
        "recv frame: ver={} type=0x{:02x} len={}",
        parsed.header.version.0, parsed.header.frame_type, parsed.header.length
    );
    if parsed.header.frame_type != ControlFrameType::ServerHello as u8 {
        bail!(
            "expected ServerHello (0x02), got 0x{:02x}",
            parsed.header.frame_type
        );
    }
    let value: Cbor = ciborium::de::from_reader(parsed.payload)
        .context("decode ServerHello CBOR")?;
    let server_caps = describe_server_hello(&value);

    let profile = match NegotiatedProfile::negotiate(&client_caps, &server_caps) {
        Ok(p) => {
            println!("Negotiated profile:");
            println!("  version:           {}", p.version);
            println!("  display_codec:     {}", p.display_codec);
            println!(
                "  display_resolution: {}x{}",
                p.display_resolution.0, p.display_resolution.1
            );
            println!("  display_max_fps:   {}", p.display_max_fps);
            println!("  audio_codec:       {}", p.audio_codec.as_deref().unwrap_or("(none)"));
            println!("  auth_methods:      {:?}", p.auth_methods);
            println!("  chat_enabled:      {}", p.chat_enabled);
            p
        }
        Err(e) => bail!("capability negotiation failed: {e}"),
    };

    // --- AuthRequest ------------------------------------------------------
    let pin = std::env::var("OPENRD_PIN").unwrap_or_else(|_| {
        eprintln!("warning: OPENRD_PIN not set; sending empty PIN (will fail auth)");
        String::new()
    });
    if !profile.auth_methods.iter().any(|m| m == "pin") {
        bail!(
            "server doesn't support PIN auth (offers {:?})",
            profile.auth_methods
        );
    }
    let auth_payload = encode_auth_request("pin", pin.as_bytes());
    let mut frame_buf = Vec::with_capacity(FrameHeader::SIZE + auth_payload.len());
    Frame::encode(
        ControlFrameType::AuthRequest as u8,
        &auth_payload,
        &mut frame_buf,
    );
    send.write_all(&frame_buf).await?;
    println!("\nsent AuthRequest (method=pin, credential={} B)", pin.len());

    let bytes = read_frame(recv).await?;
    let (parsed, _) = Frame::parse(&bytes)?;
    println!(
        "recv frame: ver={} type=0x{:02x} len={}",
        parsed.header.version.0, parsed.header.frame_type, parsed.header.length
    );
    if parsed.header.frame_type != ControlFrameType::AuthResult as u8 {
        bail!(
            "expected AuthResult (0x05), got 0x{:02x}",
            parsed.header.frame_type
        );
    }
    let auth_result: Cbor = ciborium::de::from_reader(parsed.payload)
        .context("decode AuthResult")?;
    let (status, permission, identity) = parse_auth_result(&auth_result);
    println!("AuthResult:");
    println!("  status:     {status} ({:?})", ErrorCode::from_u16(status as u16));
    println!("  permission: {permission}");
    println!("  identity:   {identity}");

    if status != 0 {
        bail!("auth failed");
    }

    // --- OpenChannel(Input) + send a few events ---------------------------
    let oc_payload = encode_open_channel(ChannelKind::INPUT.0 as u64, 1, 2);
    let mut oc_frame = Vec::with_capacity(FrameHeader::SIZE + oc_payload.len());
    Frame::encode(
        ControlFrameType::OpenChannel as u8,
        &oc_payload,
        &mut oc_frame,
    );
    send.write_all(&oc_frame).await?;
    println!("\nsent OpenChannel(Input, channel_id=1)");

    let ack_bytes = read_frame(recv).await?;
    let (ack_frame, _) = Frame::parse(&ack_bytes)?;
    if ack_frame.header.frame_type != ControlFrameType::OpenChannelAck as u8 {
        bail!(
            "expected OpenChannelAck (0x07), got 0x{:02x}",
            ack_frame.header.frame_type
        );
    }
    let ack_val: Cbor =
        ciborium::de::from_reader(ack_frame.payload).context("decode OpenChannelAck")?;
    let (ack_channel_id, ack_status) = parse_open_channel_ack(&ack_val);
    println!("recv OpenChannelAck: channel_id={ack_channel_id} status={ack_status}");
    if ack_status != 0 {
        bail!("server refused channel: status {ack_status}");
    }

    // Concurrently: send Input events (client → server) and receive
    // the Display channel test pattern (server → client).
    let input_fut = send_input_events(&conn);
    let display_fut = receive_display_test_pattern(&conn);
    let (i, d) = tokio::join!(input_fut, display_fut);
    i.context("Input task")?;
    d.context("Display task")?;

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    conn.close(0u32.into(), b"bye");
    Ok(())
}

async fn send_input_events(conn: &wtransport::Connection) -> Result<()> {
    let mut input_stream = conn.open_uni().await?.await?;
    println!("opened Input uni stream");

    let key_a_down = KeyEvent {
        keysym: 0x0061,
        scancode: 0x001E,
        modifiers: Modifiers::default(),
        flags: 0x01,
    };
    write_input_frame(&mut input_stream, InputFrameType::KeyEvent, &key_a_down).await?;
    println!("sent KeyEvent (keysym=0x61 'a' down)");

    let mut key_a_up = key_a_down;
    key_a_up.flags = 0x00;
    write_input_frame(&mut input_stream, InputFrameType::KeyEvent, &key_a_up).await?;
    println!("sent KeyEvent (keysym=0x61 'a' up)");

    let text = TextInput {
        text: "olá, mundo 🌍".to_owned(),
    };
    write_input_frame(&mut input_stream, InputFrameType::TextInput, &text).await?;
    println!("sent TextInput (\"olá, mundo 🌍\")");

    input_stream.finish().await.ok();
    println!("finished Input uni stream");
    Ok(())
}

async fn receive_display_test_pattern(conn: &wtransport::Connection) -> Result<()> {
    let mut recv = conn.accept_uni().await.context("accept Display uni")?;
    println!("accepted Display uni stream");

    let mut params_count = 0u32;
    let mut header_count = 0u32;
    let mut slice_count = 0u32;
    let mut end_count = 0u32;
    let mut total_bytes = 0u64;

    loop {
        let frame_bytes = match read_frame_opt(&mut recv).await? {
            Some(b) => b,
            None => break,
        };
        total_bytes += frame_bytes.len() as u64;
        let (frame, _) = Frame::parse(&frame_bytes)?;
        let kind = DisplayFrameType::from_u8(frame.header.frame_type)?;
        match kind {
            DisplayFrameType::StreamParameters => {
                if frame.payload.len() >= 5 {
                    let codec = frame.payload[0];
                    let width = u16::from_le_bytes([frame.payload[1], frame.payload[2]]);
                    let height = u16::from_le_bytes([frame.payload[3], frame.payload[4]]);
                    println!(
                        "Display::StreamParameters codec=0x{codec:02x} {width}x{height} ({} payload B)",
                        frame.payload.len()
                    );
                }
                params_count += 1;
            }
            DisplayFrameType::FrameHeader => {
                if frame.payload.len() >= 15 {
                    let frame_id = u32::from_le_bytes([
                        frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3],
                    ]);
                    let flags = frame.payload[4];
                    let n_slices = frame.payload[13];
                    println!(
                        "Display::FrameHeader id={frame_id} flags=0x{flags:02x} n_slices={n_slices}"
                    );
                }
                header_count += 1;
            }
            DisplayFrameType::FrameSlice => slice_count += 1,
            DisplayFrameType::FrameEnd => {
                if frame.payload.len() >= 4 {
                    let frame_id = u32::from_le_bytes([
                        frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3],
                    ]);
                    println!("Display::FrameEnd id={frame_id}");
                }
                end_count += 1;
            }
        }
    }

    println!(
        "Display stream summary: params={params_count} frames={header_count} slices={slice_count} ends={end_count} bytes={total_bytes}"
    );
    Ok(())
}

async fn read_frame_opt(recv: &mut wtransport::RecvStream) -> Result<Option<Vec<u8>>> {
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

trait WriteInputPayload {
    fn write_payload(&self, out: &mut Vec<u8>);
}
impl WriteInputPayload for KeyEvent {
    fn write_payload(&self, out: &mut Vec<u8>) {
        self.write_to(out)
    }
}
impl WriteInputPayload for TextInput {
    fn write_payload(&self, out: &mut Vec<u8>) {
        self.write_to(out)
    }
}

async fn write_input_frame<T: WriteInputPayload>(
    stream: &mut wtransport::SendStream,
    frame_type: InputFrameType,
    msg: &T,
) -> Result<()> {
    let mut payload = Vec::new();
    msg.write_payload(&mut payload);
    let mut frame = Vec::with_capacity(FrameHeader::SIZE + payload.len());
    Frame::encode(frame_type as u8, &payload, &mut frame);
    stream.write_all(&frame).await?;
    Ok(())
}

async fn read_frame(recv: &mut wtransport::RecvStream) -> Result<Vec<u8>> {
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

async fn read_exact_inner(recv: &mut wtransport::RecvStream, buf: &mut [u8]) -> Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match recv.read(&mut buf[filled..]).await? {
            Some(0) | None => bail!("stream ended early ({}/{} bytes)", filled, buf.len()),
            Some(n) => filled += n,
        }
    }
    Ok(())
}

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
            1 => {
                let n = val.as_integer().and_then(|i| u64::try_from(i).ok()).unwrap_or(0);
                println!("  protocol_version: {n}");
            }
            2 => {
                let s = val.as_text().unwrap_or("?");
                println!("  server_name:      \"{s}\"");
            }
            3 => {
                server_caps = Capabilities::from_cbor(val);
                println!("  capabilities:");
                println!("    profile:        {}", server_caps.profile);
                println!("    auth_methods:   {:?}", server_caps.auth_methods);
                println!("    display_codecs: {:?}", server_caps.display_codecs);
                println!("    audio_codecs:   {:?}", server_caps.audio_codecs);
                println!(
                    "    max_resolution: {}x{}",
                    server_caps.display_max_resolution.0,
                    server_caps.display_max_resolution.1
                );
            }
            4 => {
                let h = val.as_bytes().map(hex::encode).unwrap_or_else(|| "?".into());
                println!("  session_id:       {h}");
            }
            5 => {
                let n = val.as_integer().and_then(|i| u64::try_from(i).ok()).unwrap_or(0);
                println!("  server_time:      {n}");
            }
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
