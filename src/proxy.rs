use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::policy::Policy;
use crate::protocol::{Event, Request};

type WsTx = mpsc::UnboundedSender<Event>;

enum SocketEntry {
    Tcp {
        write_tx: mpsc::UnboundedSender<Vec<u8>>,
    },
    Listener {
        shutdown: tokio::sync::oneshot::Sender<()>,
    },
}

pub async fn handle_session(
    ws_stream: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    policy: Arc<Policy>,
) {
    let (mut ws_sink, mut ws_source) = ws_stream.split();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Event>();
    let sockets: Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let active_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let send_task = tokio::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            let json = serde_json::to_string(&ev).unwrap();
            if ws_sink.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_source.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        let req: Request = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                let _ = event_tx.send(Event::error(0, format!("invalid request: {e}")));
                continue;
            }
        };

        match req {
            Request::Connect { id, addr, port } => {
                handle_connect(
                    id,
                    addr,
                    port,
                    event_tx.clone(),
                    sockets.clone(),
                    policy.clone(),
                    active_count.clone(),
                )
                .await;
            }
            Request::Bind { id, addr, port } => {
                handle_bind(
                    id,
                    addr,
                    port,
                    event_tx.clone(),
                    sockets.clone(),
                    policy.clone(),
                )
                .await;
            }
            Request::Listen { id, backlog: _ } => {
                handle_listen(id, event_tx.clone(), sockets.clone(), active_count.clone()).await;
            }
            Request::Send { id, data } => {
                handle_send(id, data, sockets.clone()).await;
            }
            Request::Close { id } => {
                handle_close(id, sockets.clone(), active_count.clone()).await;
            }
        }
    }

    {
        let mut map = sockets.lock().await;
        for (_, entry) in map.drain() {
            if let SocketEntry::Listener { shutdown } = entry {
                let _ = shutdown.send(());
            }
        }
    }
    drop(event_tx);
    let _ = send_task.await;
}

async fn handle_connect(
    id: u64,
    addr: String,
    port: u16,
    event_tx: WsTx,
    sockets: Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>>,
    policy: Arc<Policy>,
    active_count: Arc<std::sync::atomic::AtomicUsize>,
) {
    let current = active_count.load(std::sync::atomic::Ordering::Relaxed);
    if current >= policy.max_connections {
        let _ = event_tx.send(Event::error(id, "max connections reached"));
        return;
    }

    if let Err(msg) = policy.check_connect(&addr, port) {
        let _ = event_tx.send(Event::denied(id, msg));
        return;
    }

    let timeout = Duration::from_secs(policy.connection_timeout_secs);

    tokio::spawn(async move {
        let target = format!("{addr}:{port}");
        let connect_result = tokio::time::timeout(timeout, TcpStream::connect(&target)).await;

        let stream = match connect_result {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                let _ = event_tx.send(Event::error(id, e.to_string()));
                return;
            }
            Err(_) => {
                let _ = event_tx.send(Event::error(id, "connection timed out"));
                return;
            }
        };

        active_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = event_tx.send(Event::Connected { id });

        let (write_tx, write_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        {
            let mut map = sockets.lock().await;
            map.insert(id, SocketEntry::Tcp { write_tx });
        }

        run_tcp_bridge(id, stream, event_tx, write_rx).await;

        active_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        let mut map = sockets.lock().await;
        map.remove(&id);
    });
}

async fn run_tcp_bridge(
    id: u64,
    stream: TcpStream,
    event_tx: WsTx,
    mut write_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let (mut reader, mut writer) = stream.into_split();

    let ev_tx = event_tx.clone();
    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 16384];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => {
                    let _ = ev_tx.send(Event::Closed { id });
                    break;
                }
                Ok(n) => {
                    let encoded = B64.encode(&buf[..n]);
                    let _ = ev_tx.send(Event::Data { id, data: encoded });
                }
                Err(e) => {
                    let _ = ev_tx.send(Event::error(id, e.to_string()));
                    break;
                }
            }
        }
    });

    let write_task = tokio::spawn(async move {
        while let Some(data) = write_rx.recv().await {
            if writer.write_all(&data).await.is_err() {
                break;
            }
        }
    });

    let _ = tokio::join!(read_task, write_task);
}

async fn handle_bind(
    id: u64,
    addr: String,
    port: u16,
    event_tx: WsTx,
    sockets: Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>>,
    policy: Arc<Policy>,
) {
    if let Err(msg) = policy.check_bind(port) {
        let _ = event_tx.send(Event::denied(id, msg));
        return;
    }

    let bind_addr = format!("{addr}:{port}");
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            let _ = event_tx.send(Event::error(id, e.to_string()));
            return;
        }
    };

    let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let _ = event_tx.send(Event::Listening {
        id,
        port: actual_port,
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    {
        let mut map = sockets.lock().await;
        map.insert(
            id,
            SocketEntry::Listener {
                shutdown: shutdown_tx,
            },
        );
    }

    tokio::spawn(async move {
        run_listener(id, listener, event_tx, sockets, shutdown_rx).await;
    });
}

async fn handle_listen(
    id: u64,
    event_tx: WsTx,
    _sockets: Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>>,
    _active_count: Arc<std::sync::atomic::AtomicUsize>,
) {
    let _ = event_tx.send(Event::Listening { id, port: 0 });
}

async fn run_listener(
    id: u64,
    listener: TcpListener,
    event_tx: WsTx,
    sockets: Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>>,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let next_conn_id = std::sync::atomic::AtomicU64::new(id * 1_000_000 + 1);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, remote_addr)) => {
                        let conn_id = next_conn_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = event_tx.send(Event::Accepted {
                            id,
                            conn_id,
                            remote: remote_addr.to_string(),
                        });

                        let (write_tx, write_rx) = mpsc::unbounded_channel();
                        {
                            let mut map = sockets.lock().await;
                            map.insert(conn_id, SocketEntry::Tcp { write_tx });
                        }

                        let ev_tx = event_tx.clone();
                        let socks = sockets.clone();
                        tokio::spawn(async move {
                            run_tcp_bridge(conn_id, stream, ev_tx, write_rx).await;
                            let mut map = socks.lock().await;
                            map.remove(&conn_id);
                        });
                    }
                    Err(e) => {
                        let _ = event_tx.send(Event::error(id, e.to_string()));
                        break;
                    }
                }
            }
            _ = &mut shutdown_rx => {
                break;
            }
        }
    }

    let _ = event_tx.send(Event::Closed { id });
}

async fn handle_send(
    id: u64,
    data_b64: String,
    sockets: Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>>,
) {
    let bytes = match B64.decode(&data_b64) {
        Ok(b) => b,
        Err(_) => return,
    };

    let map = sockets.lock().await;
    if let Some(SocketEntry::Tcp { write_tx }) = map.get(&id) {
        let _ = write_tx.send(bytes);
    }
}

async fn handle_close(
    id: u64,
    sockets: Arc<tokio::sync::Mutex<HashMap<u64, SocketEntry>>>,
    active_count: Arc<std::sync::atomic::AtomicUsize>,
) {
    let mut map = sockets.lock().await;
    if let Some(entry) = map.remove(&id) {
        match entry {
            SocketEntry::Tcp { .. } => {
                active_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            }
            SocketEntry::Listener { shutdown } => {
                let _ = shutdown.send(());
            }
        }
    }
}
