pub mod policy;
pub mod protocol;
pub mod proxy;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tracing::{error, info};

use policy::{NetworkPolicy, Policy, PolicyConfig};

pub struct Server {
    policy: Arc<Policy>,
    addr: SocketAddr,
}

pub struct ServerBuilder {
    policy: Option<NetworkPolicy>,
    policy_config: Option<PolicyConfig>,
    no_policy: bool,
    host: String,
    port: u16,
}

impl ServerBuilder {
    pub fn new() -> Self {
        Self {
            policy: None,
            policy_config: None,
            no_policy: false,
            host: "0.0.0.0".into(),
            port: 9000,
        }
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn addr(mut self, host: impl Into<String>, port: u16) -> Self {
        self.host = host.into();
        self.port = port;
        self
    }

    pub fn policy(mut self, policy: NetworkPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn policy_config(mut self, config: PolicyConfig) -> Self {
        self.policy_config = Some(config);
        self
    }

    pub fn policy_file(self, path: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let content = std::fs::read_to_string(path)?;
        let config: PolicyConfig = toml::from_str(&content)?;
        Ok(self.policy_config(config))
    }

    pub fn no_policy(mut self) -> Self {
        self.no_policy = true;
        self
    }

    pub fn build(self) -> Result<Server, anyhow::Error> {
        let addr: SocketAddr = format!("{}:{}", self.host, self.port).parse()?;

        let policy = if self.no_policy {
            Policy::allow_all()
        } else if let Some(p) = self.policy {
            Policy::new(&p)
        } else if let Some(c) = self.policy_config {
            Policy::new(&c.network)
        } else {
            Policy::new(&NetworkPolicy::default())
        };

        Ok(Server {
            policy: Arc::new(policy),
            addr,
        })
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn builder() -> ServerBuilder {
        ServerBuilder::new()
    }

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

    pub fn policy(&self) -> &Arc<Policy> {
        &self.policy
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn listen(self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("wasmnet listening on {}", self.addr);

        loop {
            let (stream, peer) = listener.accept().await?;
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
    }

    pub async fn listen_with_shutdown(
        self,
        shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("wasmnet listening on {}", self.addr);

        tokio::select! {
            result = async {
                loop {
                    let (stream, peer) = listener.accept().await?;
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
                #[allow(unreachable_code)]
                Ok::<(), std::io::Error>(())
            } => { result }
            _ = shutdown => {
                info!("wasmnet shutting down");
                Ok(())
            }
        }
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

pub fn load_policy_file(path: impl AsRef<Path>) -> Result<PolicyConfig, anyhow::Error> {
    let content = std::fs::read_to_string(path)?;
    let config: PolicyConfig = toml::from_str(&content)?;
    Ok(config)
}
