//! OpenRD reference server (Linux primary; runs on any host for dev).
//!
//! v0 status: scaffolding. The server stands up a QUIC endpoint with
//! ALPN `openrd/v0`, accepts connections, accepts the Control
//! bidirectional stream from the client, runs the hello exchange,
//! and then closes. The full Control loop, capture, encode, and
//! input injection are TODO.
//!
//! Run with:
//! ```text
//! RUST_LOG=openrd_server=info cargo run -p openrd-server
//! ```

mod control;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use quinn::{Endpoint, ServerConfig};
use rand::Rng;
use tracing::{info, warn};

use openrd_proto::ALPN;

const LISTEN_ADDR: &str = "[::]:4443";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openrd_server=info,openrd_proto=info".into()),
        )
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let listen: SocketAddr = LISTEN_ADDR.parse()?;
    let server_config = make_server_config()?;
    let endpoint = Endpoint::server(server_config, listen)?;

    // Generate a 9-digit PIN for this server instance. Stable across
    // connections during the server's lifetime.
    let pin: String = format!("{:09}", rand::rng().random_range(0..1_000_000_000u32));
    let pin = Arc::new(pin);
    info!(pin = %pin, "OpenRD PIN issued for this server instance");

    info!("openrd-server listening on {listen} (ALPN openrd/v0)");

    while let Some(incoming) = endpoint.accept().await {
        let pin = Arc::clone(&pin);
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    if let Err(e) = handle_connection(conn, pin).await {
                        warn!("connection error: {e:#}");
                    }
                }
                Err(e) => warn!("connection setup failed: {e}"),
            }
        });
    }

    Ok(())
}

async fn handle_connection(conn: quinn::Connection, pin: Arc<String>) -> Result<()> {
    let remote = conn.remote_address();
    info!(%remote, "new connection");

    // Accept the Control bidirectional stream from the client.
    let (send, recv) = conn
        .accept_bi()
        .await
        .context("accept Control bidi stream")?;
    info!(%remote, "accepted Control bidi stream");

    if let Err(e) = control::handle_control_stream(send, recv, remote, &pin).await {
        warn!(%remote, "Control flow failed: {e:#}");
    }

    conn.closed().await;
    info!(%remote, "connection closed");
    Ok(())
}

fn make_server_config() -> Result<ServerConfig> {
    // Dev-only self-signed certificate. Production deployments use a
    // real cert from the operator's PKI (file path, ACME, etc.).
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = cert.cert.der().clone();
    let key_der = rustls::pki_types::PrivatePkcs8KeyDer::from(
        cert.key_pair.serialize_der(),
    );

    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert_der],
            rustls::pki_types::PrivateKeyDer::Pkcs8(key_der),
        )?;
    crypto.alpn_protocols = vec![ALPN.to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?;
    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_crypto));

    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_uni_streams(64u32.into());
    transport.max_concurrent_bidi_streams(64u32.into());
    server_config.transport_config(Arc::new(transport));

    Ok(server_config)
}
