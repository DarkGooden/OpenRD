//! OpenRD reference server (Linux primary; runs on any host for dev).
//!
//! v0 status: scaffolding. Accepts WebTransport sessions at `/openrd`,
//! runs hello + PIN auth + one Input channel exchange, then closes.
//!
//! Run with:
//! ```text
//! RUST_LOG=openrd_server=info cargo run -p openrd-server
//! ```

mod control;

use std::sync::Arc;

use anyhow::Result;
use rand::Rng;
use tracing::{info, warn};
use wtransport::{Endpoint, Identity, ServerConfig};

const LISTEN_PORT: u16 = 4443;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openrd_server=info,openrd_proto=info".into()),
        )
        .init();

    // Self-signed dev cert. SANs include 'localhost' and '127.0.0.1' so
    // WebTransport (which requires a hostname match) is happy.
    let identity = Identity::self_signed(["localhost", "127.0.0.1"])?;

    // Print the cert hash so web clients (which need
    // serverCertificateHashes to accept self-signed certs) can pin it.
    let chain = identity.certificate_chain();
    if let Some(cert) = chain.as_slice().first() {
        let hash = cert.hash();
        info!(cert_sha256 = %hash.fmt(wtransport::tls::Sha256DigestFmt::BytesArray), "server cert hash (for WebTransport pinning)");
        info!(cert_sha256_dotted = %hash.fmt(wtransport::tls::Sha256DigestFmt::DottedHex), "server cert hash dotted-hex");
    }

    let config = ServerConfig::builder()
        .with_bind_default(LISTEN_PORT)
        .with_identity(identity)
        .build();
    let server = Endpoint::server(config)?;

    // 9-digit PIN, stable for the server's lifetime.
    let pin: String = format!("{:09}", rand::rng().random_range(0..1_000_000_000u32));
    let pin = Arc::new(pin);
    info!(pin = %pin, "OpenRD PIN issued for this server instance");
    info!("openrd-server listening on UDP/{LISTEN_PORT} (WebTransport /openrd)");

    loop {
        let incoming = server.accept().await;
        let pin = Arc::clone(&pin);
        tokio::spawn(async move {
            if let Err(e) = serve_session(incoming, pin).await {
                warn!("session error: {e:#}");
            }
        });
    }
}

async fn serve_session(
    incoming: wtransport::endpoint::IncomingSession,
    pin: Arc<String>,
) -> Result<()> {
    let session_req = incoming.await?;
    let path = session_req.path().to_string();
    let conn = session_req.accept().await?;
    let remote = conn.remote_address();
    info!(%remote, path = %path, "new WebTransport session");

    // v0 expects "/openrd"; tolerate variations during scaffolding.
    if !path.starts_with("/openrd") {
        warn!(%remote, path = %path, "unexpected path (expected /openrd)");
    }

    let (send, recv) = conn.accept_bi().await?;
    info!(%remote, "accepted Control bidi stream");

    if let Err(e) = control::handle_control_stream(&conn, send, recv, remote, &pin).await {
        warn!(%remote, "Control flow failed: {e:#}");
    }

    info!(%remote, "session ending");
    Ok(())
}
