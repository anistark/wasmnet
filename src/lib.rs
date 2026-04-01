pub mod binary;
pub mod dns;
pub mod policy;
pub mod pool;
pub mod protocol;
pub mod proxy;
pub mod rate_limit;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tracing::{error, info};

use policy::{NetworkPolicy, Policy, PolicyConfig};
use pool::ConnectionPool;
use proxy::SessionConfig;
use rate_limit::RateLimiter;

pub struct Server {
    policy: Arc<Policy>,
    rate_limiter: Arc<RateLimiter>,
    pool: Option<Arc<ConnectionPool>>,
    addr: SocketAddr,
}

pub struct ServerBuilder {
    policy: Option<NetworkPolicy>,
    policy_config: Option<PolicyConfig>,
    no_policy: bool,
    host: String,
    port: u16,
    max_bandwidth_mbps: Option<u32>,
    pool_idle_secs: Option<u64>,
    pool_per_key: Option<usize>,
}

impl ServerBuilder {
    pub fn new() -> Self {
        Self {
            policy: None,
            policy_config: None,
            no_policy: false,
            host: "0.0.0.0".into(),
            port: 9000,
            max_bandwidth_mbps: None,
            pool_idle_secs: None,
            pool_per_key: None,
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

    pub fn max_bandwidth_mbps(mut self, mbps: u32) -> Self {
        self.max_bandwidth_mbps = Some(mbps);
        self
    }

    pub fn pool(mut self, idle_secs: u64, per_key: usize) -> Self {
        self.pool_idle_secs = Some(idle_secs);
        self.pool_per_key = Some(per_key);
        self
    }

    pub fn build(self) -> Result<Server, anyhow::Error> {
        let addr: SocketAddr = format!("{}:{}", self.host, self.port).parse()?;

        let net_policy = if self.no_policy {
            None
        } else if let Some(p) = self.policy {
            Some(p)
        } else if let Some(c) = self.policy_config {
            Some(c.network)
        } else {
            Some(NetworkPolicy::default())
        };

        let policy = match &net_policy {
            Some(np) => Policy::new(np),
            None => Policy::allow_all(),
        };

        let bandwidth = self
            .max_bandwidth_mbps
            .or(net_policy.as_ref().map(|p| p.max_bandwidth_mbps))
            .unwrap_or(0);

        let rate_limiter = if bandwidth > 0 {
            RateLimiter::new(bandwidth)
        } else {
            RateLimiter::unlimited()
        };

        let pool = match (self.pool_idle_secs, self.pool_per_key) {
            (Some(idle), Some(per_key)) => {
                let p = Arc::new(ConnectionPool::new(idle, per_key));
                p.start_cleanup_task();
                Some(p)
            }
            _ => None,
        };

        Ok(Server {
            policy: Arc::new(policy),
            rate_limiter: Arc::new(rate_limiter),
            pool,
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
        let bw = policy.max_bandwidth_mbps;
        Self {
            policy: Arc::new(Policy::new(&policy)),
            rate_limiter: Arc::new(if bw > 0 {
                RateLimiter::new(bw)
            } else {
                RateLimiter::unlimited()
            }),
            pool: None,
            addr,
        }
    }

    pub fn from_config(config: PolicyConfig, addr: SocketAddr) -> Self {
        Self::new(config.network, addr)
    }

    pub fn allow_all(addr: SocketAddr) -> Self {
        Self {
            policy: Arc::new(Policy::allow_all()),
            rate_limiter: Arc::new(RateLimiter::unlimited()),
            pool: None,
            addr,
        }
    }

    pub fn policy(&self) -> &Arc<Policy> {
        &self.policy
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn session_config(&self) -> SessionConfig {
        SessionConfig {
            policy: self.policy.clone(),
            rate_limiter: self.rate_limiter.clone(),
            pool: self.pool.clone(),
        }
    }

    pub async fn listen(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("wasmnet listening on {}", self.addr);

        loop {
            let (stream, peer) = listener.accept().await?;
            let config = self.session_config();
            tokio::spawn(async move {
                match accept_async(stream).await {
                    Ok(ws) => {
                        info!("new session from {peer}");
                        proxy::handle_session(ws, config).await;
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
        &self,
        shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("wasmnet listening on {}", self.addr);

        tokio::select! {
            result = async {
                loop {
                    let (stream, peer) = listener.accept().await?;
                    let config = self.session_config();
                    tokio::spawn(async move {
                        match accept_async(stream).await {
                            Ok(ws) => {
                                info!("new session from {peer}");
                                proxy::handle_session(ws, config).await;
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
    let config = SessionConfig {
        policy,
        rate_limiter: Arc::new(RateLimiter::unlimited()),
        pool: None,
    };
    match accept_async(stream).await {
        Ok(ws) => {
            proxy::handle_session(ws, config).await;
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
