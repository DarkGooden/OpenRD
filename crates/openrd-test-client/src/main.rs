//! Native test client for the OpenRD reference server.
//!
//! Connects to a local `openrd-server` via raw QUIC, opens the Control
//! bidirectional stream, sends a `ClientHello`, reads the `ServerHello`,
//! and prints the parsed fields. Then exits.
//!
//! **Dev only.** The client skips server-certificate validation so it
//! works against the server's self-signed dev cert. Do not use any
//! pattern from this file in production code.
//!
//! Run with:
//! ```text
//! cargo run -p openrd-test-client
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use ciborium::Value as Cbor;
use openrd_proto::control::ControlFrameType;
use openrd_proto::{
    Capabilities, Frame, FrameHeader, NegotiatedProfile, ALPN, MAX_FRAME_LENGTH,
    PROTOCOL_VERSION,
};
use quinn::{ClientConfig, Endpoint};

const SERVER: &str = "127.0.0.1:4443";
const SNI: &str = "localhost";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let mut endpoint = Endpoint::client("[::]:0".parse()?)?;
    endpoint.set_default_client_config(make_client_config()?);

    let server_addr: SocketAddr = SERVER.parse()?;
    println!("connecting to {server_addr} (SNI {SNI})...");

    let conn = endpoint
        .connect(server_addr, SNI)?
        .await
        .context("QUIC connect")?;
    println!("connected; opening Control bidirectional stream");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;

    let client_caps = Capabilities::default();

    // ClientHello.
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

    // ServerHello.
    let bytes = read_frame(&mut recv).await?;
    let (parsed, _) = Frame::parse(&bytes)?;
    println!(
        "recv frame: ver={} type=0x{:02x} len={}",
        parsed.header.version.0, parsed.header.frame_type, parsed.header.length
    );

    if parsed.header.frame_type == ControlFrameType::Error as u8 {
        let v: Cbor = ciborium::de::from_reader(parsed.payload)
            .context("decode Error frame")?;
        bail!("server returned Error frame: {v:?}");
    }
    if parsed.header.frame_type != ControlFrameType::ServerHello as u8 {
        bail!(
            "expected ServerHello (0x02), got 0x{:02x}",
            parsed.header.frame_type
        );
    }

    let value: Cbor = ciborium::de::from_reader(parsed.payload)
        .context("decode ServerHello CBOR")?;
    let server_caps = describe_server_hello(&value);

    // Compute the negotiated profile and print it.
    match NegotiatedProfile::negotiate(&client_caps, &server_caps) {
        Ok(profile) => {
            println!("Negotiated profile:");
            println!("  version:           {}", profile.version);
            println!("  display_codec:     {}", profile.display_codec);
            println!("  display_resolution: {}x{}", profile.display_resolution.0, profile.display_resolution.1);
            println!("  display_max_fps:   {}", profile.display_max_fps);
            println!("  audio_codec:       {}", profile.audio_codec.as_deref().unwrap_or("(none)"));
            println!("  auth_methods:      {:?}", profile.auth_methods);
            println!("  chat_enabled:      {}", profile.chat_enabled);
        }
        Err(e) => {
            anyhow::bail!("capability negotiation failed: {e}");
        }
    }

    conn.close(0u32.into(), b"bye");
    endpoint.wait_idle().await;
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

async fn read_frame(recv: &mut quinn::RecvStream) -> Result<Vec<u8>> {
    let mut header_buf = [0u8; FrameHeader::SIZE];
    recv.read_exact(&mut header_buf).await?;
    let h = FrameHeader::parse(&header_buf)?;
    if (h.length as usize) > MAX_FRAME_LENGTH {
        bail!("oversized frame: {} bytes", h.length);
    }
    let mut full = vec![0u8; FrameHeader::SIZE + h.length as usize];
    full[..FrameHeader::SIZE].copy_from_slice(&header_buf);
    recv.read_exact(&mut full[FrameHeader::SIZE..]).await?;
    Ok(full)
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
                let h = val
                    .as_bytes()
                    .map(|b| hex::encode(b))
                    .unwrap_or_else(|| "?".into());
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

fn make_client_config() -> Result<ClientConfig> {
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipVerify))
        .with_no_client_auth();
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?;
    Ok(ClientConfig::new(Arc::new(quic_crypto)))
}

#[derive(Debug)]
struct SkipVerify;

impl rustls::client::danger::ServerCertVerifier for SkipVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}
