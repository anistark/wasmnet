//! WebTransport (HTTP/3 over QUIC) transport.
//!
//! Carries the same request/event protocol as the WebSocket transport. The
//! browser opens one bidirectional stream; messages are length-prefixed
//! (`[4B big-endian length][binary frame]`) and reuse the [`crate::binary`]
//! codec. Behind the `webtransport` cargo feature.

use std::net::SocketAddr;
use std::path::Path;

use tracing::{debug, info, warn};
use wtransport::endpoint::IncomingSession;
use wtransport::stream::{RecvStream, SendStream};
use wtransport::tls::Sha256DigestFmt;
use wtransport::{Endpoint, Identity, ServerConfig};

use crate::binary;
use crate::protocol::Event;
use crate::proxy::{self, Ctx, SessionConfig};

/// Reject length prefixes above this (16 MiB) to bound per-frame allocation.
const MAX_FRAME: u32 = 16 * 1024 * 1024;

/// Build the endpoint's TLS identity. Supplying both `cert` and `key` loads a
/// PEM keypair; supplying neither generates a self-signed certificate (dev
/// only) and logs its SHA-256 for the browser's `serverCertificateHashes`.
pub async fn build_identity(cert: Option<&Path>, key: Option<&Path>) -> anyhow::Result<Identity> {
    match (cert, key) {
        (Some(cert), Some(key)) => Ok(Identity::load_pemfiles(cert, key).await?),
        (None, None) => {
            let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"])?;
            warn!("WebTransport: using a generated self-signed certificate (dev only)");
            if let Some(c) = identity.certificate_chain().as_slice().first() {
                info!(
                    "WebTransport cert SHA-256: {}",
                    c.hash().fmt(Sha256DigestFmt::DottedHex)
                );
            }
            Ok(identity)
        }
        _ => anyhow::bail!("WebTransport needs both a cert and key, or neither"),
    }
}

/// Run the WebTransport endpoint until the task is dropped. `config` is cloned
/// per accepted session, mirroring the WebSocket listener.
pub async fn serve(
    addr: SocketAddr,
    identity: Identity,
    config: SessionConfig,
) -> anyhow::Result<()> {
    let server_config = ServerConfig::builder()
        .with_bind_address(addr)
        .with_identity(identity)
        .build();
    let endpoint = Endpoint::server(server_config)?;
    info!("wasmnet WebTransport listening on {addr}");

    loop {
        let incoming = endpoint.accept().await;
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(e) = accept_session(incoming, config).await {
                debug!("WebTransport session ended: {e}");
            }
        });
    }
}

async fn accept_session(incoming: IncomingSession, config: SessionConfig) -> anyhow::Result<()> {
    let request = incoming.await?;
    debug!("WebTransport request path={}", request.path());
    let connection = request.accept().await?;
    let (send, recv) = connection.accept_bi().await?;
    run_session(send, recv, config).await;
    Ok(())
}

async fn run_session(mut send: SendStream, mut recv: RecvStream, config: SessionConfig) {
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    let ctx = Ctx::new(event_tx, config);

    let send_task = tokio::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            let body = binary::encode_event(&ev);
            if send
                .write_all(&(body.len() as u32).to_be_bytes())
                .await
                .is_err()
                || send.write_all(&body).await.is_err()
            {
                break;
            }
        }
        let _ = send.finish().await;
    });

    let mut len_buf = [0u8; 4];
    while recv.read_exact(&mut len_buf).await.is_ok() {
        let len = u32::from_be_bytes(len_buf);
        if len == 0 || len > MAX_FRAME {
            break;
        }
        let mut frame = vec![0u8; len as usize];
        if recv.read_exact(&mut frame).await.is_err() {
            break;
        }
        match binary::decode_request(&frame) {
            Ok(req) => proxy::dispatch(req, &ctx).await,
            Err(e) => ctx.notify(Event::error(0, format!("invalid binary frame: {e}"))),
        }
    }

    proxy::cleanup(&ctx).await;
    drop(ctx);
    let _ = send_task.await;
}
