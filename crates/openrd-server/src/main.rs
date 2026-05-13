//! OpenRD reference server (Linux).
//!
//! v0 status: scaffolding. The server stands up a QUIC endpoint with
//! ALPN `openrd/v0`, accepts connections, and logs them. The Control
//! channel handler, channel dispatch, capture, encode, and input
//! injection are TODO and tracked in the docs.
//!
//! Run with:
//! ```text
//! RUST_LOG=openrd_server=info cargo run -p openrd-server
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use quinn::{Endpoint, ServerConfig};
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
    info!("openrd-server listening on {listen} (ALPN openrd/v0)");

    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    if let Err(e) = handle_connection(conn).await {
                        warn!("connection error: {e:#}");
                    }
                }
                Err(e) => warn!("connection setup failed: {e}"),
            }
        });
    }

    Ok(())
}

async fn handle_connection(conn: quinn::Connection) -> Result<()> {
    info!(remote = %conn.remote_address(), "new connection");

    // TODO: accept the Control bidirectional stream (QUIC stream id 0)
    // from the client. Run the hello exchange (ClientHello /
    // ServerHello). Authenticate. Loop on Control messages: dispatch
    // OpenChannel requests, route channel data, etc.
    //
    // The state machine is in docs/21-state-machines.md.
    // The Control message schemas are in
    //   crates/openrd-proto/src/control.rs
    //   docs/20-wire-format-v0.md
    // The capability schema is in docs/22-capability-negotiation.md.

    conn.closed().await;
    info!(remote = %conn.remote_address(), "connection closed");
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
