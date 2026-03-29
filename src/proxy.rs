use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::debug;

use crate::binary;
use crate::dns;
use crate::policy::Policy;
use crate::pool::ConnectionPool;
use crate::protocol::{Event, Request};
use crate::rate_limit::RateLimiter;

type EventTx = mpsc::UnboundedSender<Event>;
type SocketMap = Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>>;

enum SocketEntry {
    Tcp {
        write_tx: mpsc::UnboundedSender<Vec<u8>>,
    },
    Udp {
        socket: Arc<UdpSocket>,
        default_target: Option<String>,
        _cancel: tokio::sync::oneshot::Sender<()>,
    },
    Listener {
        shutdown: tokio::sync::oneshot::Sender<()>,
    },
}

pub struct SessionConfig {
    pub policy: Arc<Policy>,
    pub rate_limiter: Arc<RateLimiter>,
    pub pool: Option<Arc<ConnectionPool>>,
}

#[derive(Clone)]
struct Ctx {
    tx: EventTx,
    sockets: SocketMap,
    policy: Arc<Policy>,
    active: Arc<AtomicUsize>,
    rl: Arc<RateLimiter>,
    pool: Option<Arc<ConnectionPool>>,
}

impl Ctx {
    fn check_limits(&self, id: u64, addr: &str, port: u16) -> bool {
        if self.active.load(Ordering::Relaxed) >= self.policy.max_connections {
            let _ = self.tx.send(Event::error(id, "max connections reached"));
            return true;
        }
        if let Err(msg) = self.policy.check_connect(addr, port) {
            let _ = self.tx.send(Event::denied(id, msg));
            return true;
        }
        false
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.policy.connection_timeout_secs)
    }
}

pub async fn handle_session(
    ws_stream: tokio_tungstenite::WebSocketStream<TcpStream>,
    config: SessionConfig,
) {
    let (mut ws_sink, mut ws_source) = ws_stream.split();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Event>();
    let binary_mode = Arc::new(AtomicBool::new(false));

    let ctx = Ctx {
        tx: event_tx,
        sockets: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        policy: config.policy,
        active: Arc::new(AtomicUsize::new(0)),
        rl: config.rate_limiter,
        pool: config.pool,
    };

    let bm = binary_mode.clone();
    let send_task = tokio::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            let msg = if bm.load(Ordering::Relaxed) {
                Message::Binary(binary::encode_event(&ev).into())
            } else {
                Message::Text(serde_json::to_string(&ev).unwrap().into())
            };
            if ws_sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_source.next().await {
        let req = match msg {
            Message::Text(t) => {
                binary_mode.store(false, Ordering::Relaxed);
                match serde_json::from_str::<Request>(&t) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = ctx
                            .tx
                            .send(Event::error(0, format!("invalid request: {e}")));
                        continue;
                    }
                }
            }
            Message::Binary(b) => {
                binary_mode.store(true, Ordering::Relaxed);
                match binary::decode_request(&b) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = ctx
                            .tx
                            .send(Event::error(0, format!("invalid binary frame: {e}")));
                        continue;
                    }
                }
            }
            Message::Close(_) => break,
            _ => continue,
        };

        match req {
            Request::Connect { id, addr, port } => handle_connect(id, addr, port, &ctx),
            Request::ConnectTls { id, addr, port } => handle_connect_tls(id, addr, port, &ctx),
            Request::ConnectUdp { id, addr, port } => {
                handle_connect_udp(id, addr, port, &ctx).await;
            }
            Request::Bind { id, addr, port } => handle_bind(id, addr, port, &ctx).await,
            Request::Listen { id, .. } => {
                let _ = ctx.tx.send(Event::Listening { id, port: 0 });
            }
            Request::Send { id, data } => handle_send(id, data, &ctx.sockets).await,
            Request::SendTo {
                id,
                addr,
                port,
                data,
            } => handle_send_to(id, addr, port, data, &ctx.sockets).await,
            Request::Close { id } => handle_close(id, &ctx.sockets, &ctx.active).await,
            Request::Resolve { id, name } => handle_resolve(id, name, ctx.tx.clone()),
        }
    }

    {
        let mut map = ctx.sockets.lock().await;
        for (_, entry) in map.drain() {
            if let SocketEntry::Listener { shutdown } = entry {
                let _ = shutdown.send(());
            }
        }
    }
    drop(ctx);
    let _ = send_task.await;
}

// ── TCP connect ──────────────────────────────────────────

fn handle_connect(id: u64, addr: String, port: u16, ctx: &Ctx) {
    if ctx.check_limits(id, &addr, port) {
        return;
    }
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let target = format!("{addr}:{port}");

        let stream = match try_pool_or_connect(&ctx.pool, &target, ctx.timeout()).await {
            Ok(s) => s,
            Err(msg) => {
                let _ = ctx.tx.send(Event::error(id, msg));
                return;
            }
        };

        ctx.active.fetch_add(1, Ordering::Relaxed);
        let _ = ctx.tx.send(Event::Connected { id });

        let (write_tx, write_rx) = mpsc::unbounded_channel();
        ctx.sockets
            .lock()
            .await
            .insert(id, SocketEntry::Tcp { write_tx });

        let (r, w) = stream.into_split();
        run_bridge(id, r, w, ctx.tx.clone(), write_rx, ctx.rl.clone()).await;

        ctx.active.fetch_sub(1, Ordering::Relaxed);
        ctx.sockets.lock().await.remove(&id);
    });
}

// ── TLS connect ──────────────────────────────────────────

fn handle_connect_tls(id: u64, addr: String, port: u16, ctx: &Ctx) {
    if ctx.check_limits(id, &addr, port) {
        return;
    }
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let target = format!("{addr}:{port}");

        let tcp = match tcp_connect(&target, ctx.timeout()).await {
            Ok(s) => s,
            Err(msg) => {
                let _ = ctx.tx.send(Event::error(id, msg));
                return;
            }
        };

        let tls = match tls_handshake(&addr, tcp).await {
            Ok(s) => s,
            Err(e) => {
                let _ = ctx.tx.send(Event::error(id, format!("TLS error: {e}")));
                return;
            }
        };

        ctx.active.fetch_add(1, Ordering::Relaxed);
        let _ = ctx.tx.send(Event::Connected { id });

        let (write_tx, write_rx) = mpsc::unbounded_channel();
        ctx.sockets
            .lock()
            .await
            .insert(id, SocketEntry::Tcp { write_tx });

        let (r, w) = tokio::io::split(tls);
        run_bridge(id, r, w, ctx.tx.clone(), write_rx, ctx.rl.clone()).await;

        ctx.active.fetch_sub(1, Ordering::Relaxed);
        ctx.sockets.lock().await.remove(&id);
    });
}

async fn tls_handshake(
    domain: &str,
    tcp: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, Box<dyn std::error::Error + Send + Sync>> {
    use rustls::pki_types::ServerName;
    use tokio_rustls::TlsConnector;

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    );

    let connector = TlsConnector::from(config);
    let name = ServerName::try_from(domain.to_string())?;
    Ok(connector.connect(name, tcp).await?)
}

// ── UDP ──────────────────────────────────────────────────

async fn handle_connect_udp(id: u64, addr: String, port: u16, ctx: &Ctx) {
    if ctx.check_limits(id, &addr, port) {
        return;
    }

    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            let _ = ctx
                .tx
                .send(Event::error(id, format!("UDP bind failed: {e}")));
            return;
        }
    };

    let target = format!("{addr}:{port}");
    if let Err(e) = socket.connect(&target).await {
        let _ = ctx
            .tx
            .send(Event::error(id, format!("UDP connect failed: {e}")));
        return;
    }

    let local_port = socket.local_addr().map(|a| a.port()).unwrap_or(0);
    let socket = Arc::new(socket);

    ctx.active.fetch_add(1, Ordering::Relaxed);

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    ctx.sockets.lock().await.insert(
        id,
        SocketEntry::Udp {
            socket: socket.clone(),
            default_target: Some(target),
            _cancel: cancel_tx,
        },
    );

    let _ = ctx.tx.send(Event::UdpBound {
        id,
        port: local_port,
    });

    let tx = ctx.tx.clone();
    let sock = socket.clone();
    let rl = ctx.rl.clone();
    let sockets = ctx.sockets.clone();
    let active = ctx.active.clone();

    tokio::spawn(async move {
        let mut cancel_rx = cancel_rx;
        let mut buf = [0u8; 65536];
        loop {
            tokio::select! {
                result = sock.recv_from(&mut buf) => {
                    match result {
                        Ok((n, from)) => {
                            rl.consume(n).await;
                            let _ = tx.send(Event::DataFrom {
                                id,
                                data: B64.encode(&buf[..n]),
                                addr: from.ip().to_string(),
                                port: from.port(),
                            });
                        }
                        Err(_) => break,
                    }
                }
                _ = &mut cancel_rx => break,
            }
        }
        active.fetch_sub(1, Ordering::Relaxed);
        sockets.lock().await.remove(&id);
        let _ = tx.send(Event::Closed { id });
    });
}

// ── Bind / Listen ────────────────────────────────────────

async fn handle_bind(id: u64, addr: String, port: u16, ctx: &Ctx) {
    if let Err(msg) = ctx.policy.check_bind(port) {
        let _ = ctx.tx.send(Event::denied(id, msg));
        return;
    }

    let bind_addr = format!("{addr}:{port}");
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            let _ = ctx.tx.send(Event::error(id, e.to_string()));
            return;
        }
    };

    let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let _ = ctx.tx.send(Event::Listening {
        id,
        port: actual_port,
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    ctx.sockets.lock().await.insert(
        id,
        SocketEntry::Listener {
            shutdown: shutdown_tx,
        },
    );

    let tx = ctx.tx.clone();
    let sockets = ctx.sockets.clone();
    tokio::spawn(async move {
        run_listener(id, listener, tx, sockets, shutdown_rx).await;
    });
}

async fn run_listener(
    id: u64,
    listener: TcpListener,
    event_tx: EventTx,
    sockets: SocketMap,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let next_conn_id = std::sync::atomic::AtomicU64::new(id * 1_000_000 + 1);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, remote_addr)) => {
                        let conn_id = next_conn_id.fetch_add(1, Ordering::Relaxed);
                        let _ = event_tx.send(Event::Accepted {
                            id,
                            conn_id,
                            remote: remote_addr.to_string(),
                        });

                        let (write_tx, write_rx) = mpsc::unbounded_channel();
                        sockets.lock().await.insert(conn_id, SocketEntry::Tcp { write_tx });

                        let ev_tx = event_tx.clone();
                        let socks = sockets.clone();
                        let rl = Arc::new(RateLimiter::unlimited());
                        tokio::spawn(async move {
                            let (r, w) = stream.into_split();
                            run_bridge(conn_id, r, w, ev_tx, write_rx, rl).await;
                            socks.lock().await.remove(&conn_id);
                        });
                    }
                    Err(e) => {
                        let _ = event_tx.send(Event::error(id, e.to_string()));
                        break;
                    }
                }
            }
            _ = &mut shutdown_rx => break,
        }
    }

    let _ = event_tx.send(Event::Closed { id });
}

// ── Send / SendTo ────────────────────────────────────────

async fn handle_send(id: u64, data_b64: String, sockets: &SocketMap) {
    let bytes = match B64.decode(&data_b64) {
        Ok(b) => b,
        Err(_) => return,
    };

    let udp_action = {
        let map = sockets.lock().await;
        match map.get(&id) {
            Some(SocketEntry::Tcp { write_tx }) => {
                let _ = write_tx.send(bytes);
                return;
            }
            Some(SocketEntry::Udp {
                socket,
                default_target,
                ..
            }) => Some((socket.clone(), default_target.clone())),
            _ => None,
        }
    };

    if let Some((socket, Some(target))) = udp_action {
        let _ = socket.send_to(&bytes, &target).await;
    }
}

async fn handle_send_to(id: u64, addr: String, port: u16, data_b64: String, sockets: &SocketMap) {
    let bytes = match B64.decode(&data_b64) {
        Ok(b) => b,
        Err(_) => return,
    };

    let socket = {
        let map = sockets.lock().await;
        match map.get(&id) {
            Some(SocketEntry::Udp { socket, .. }) => Some(socket.clone()),
            _ => None,
        }
    };

    if let Some(socket) = socket {
        let _ = socket.send_to(&bytes, format!("{addr}:{port}")).await;
    }
}

// ── Close ────────────────────────────────────────────────

async fn handle_close(id: u64, sockets: &SocketMap, active: &Arc<AtomicUsize>) {
    let mut map = sockets.lock().await;
    if let Some(entry) = map.remove(&id) {
        match entry {
            SocketEntry::Tcp { .. } | SocketEntry::Udp { .. } => {
                active.fetch_sub(1, Ordering::Relaxed);
            }
            SocketEntry::Listener { shutdown } => {
                let _ = shutdown.send(());
            }
        }
    }
}

// ── DNS resolve ──────────────────────────────────────────

fn handle_resolve(id: u64, name: String, tx: EventTx) {
    tokio::spawn(async move {
        match dns::resolve(&name).await {
            Ok(addrs) => {
                let _ = tx.send(Event::Resolved { id, addrs });
            }
            Err(msg) => {
                let _ = tx.send(Event::error(id, msg));
            }
        }
    });
}

// ── Bridge (generic AsyncRead + AsyncWrite) ──────────────

async fn run_bridge<R, W>(
    id: u64,
    reader: R,
    writer: W,
    event_tx: EventTx,
    mut write_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    rate_limiter: Arc<RateLimiter>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let ev_tx = event_tx.clone();
    let rl_read = rate_limiter.clone();

    let read_task = tokio::spawn(async move {
        let mut reader = reader;
        let mut buf = [0u8; 16384];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => {
                    let _ = ev_tx.send(Event::Closed { id });
                    break;
                }
                Ok(n) => {
                    rl_read.consume(n).await;
                    let _ = ev_tx.send(Event::Data {
                        id,
                        data: B64.encode(&buf[..n]),
                    });
                }
                Err(e) => {
                    let _ = ev_tx.send(Event::error(id, e.to_string()));
                    break;
                }
            }
        }
    });

    let write_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(data) = write_rx.recv().await {
            rate_limiter.consume(data.len()).await;
            if writer.write_all(&data).await.is_err() {
                break;
            }
        }
    });

    let _ = tokio::join!(read_task, write_task);
}

// ── Helpers ──────────────────────────────────────────────

async fn tcp_connect(target: &str, timeout: Duration) -> Result<TcpStream, String> {
    match tokio::time::timeout(timeout, TcpStream::connect(target)).await {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("connection timed out".into()),
    }
}

async fn try_pool_or_connect(
    pool: &Option<Arc<ConnectionPool>>,
    target: &str,
    timeout: Duration,
) -> Result<TcpStream, String> {
    if let Some(pool) = pool {
        if let Some(s) = pool.get(target).await {
            debug!("reusing pooled connection to {target}");
            return Ok(s);
        }
    }
    tcp_connect(target, timeout).await
}
