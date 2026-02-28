pub mod policy;
pub mod protocol;
pub mod proxy;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tracing::{error, info};

use policy::{NetworkPolicy, Policy, PolicyConfig};

pub struct Server {
    policy: Arc<Policy>,
    addr: SocketAddr,
}

impl Server {
    pub fn new(policy: NetworkPolicy, addr: SocketAddr) -> Self {
        Self {
            policy: Arc::new(Policy::new(&policy)),
            addr,
        }
    }

    pub fn from_config(config: PolicyConfig, addr: SocketAddr) -> Self {
        Self::new(config.network, addr)
    }

    pub fn allow_all(addr: SocketAddr) -> Self {
        Self {
            policy: Arc::new(Policy::allow_all()),
            addr,
        }
    }

    pub async fn listen(self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("wasmnet listening on {}", self.addr);

        while let Ok((stream, peer)) = listener.accept().await {
            let policy = self.policy.clone();
            tokio::spawn(async move {
                match accept_async(stream).await {
                    Ok(ws) => {
                        info!("new session from {peer}");
                        proxy::handle_session(ws, policy).await;
                        info!("session ended: {peer}");
                    }
                    Err(e) => {
                        error!("websocket handshake failed for {peer}: {e}");
                    }
                }
            });
        }

        Ok(())
    }
}

pub async fn handle_ws_upgrade(stream: tokio::net::TcpStream, policy: Arc<Policy>) {
    match accept_async(stream).await {
        Ok(ws) => {
            proxy::handle_session(ws, policy).await;
        }
        Err(e) => {
            error!("websocket upgrade failed: {e}");
        }
    }
}
