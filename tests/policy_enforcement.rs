//! End-to-end checks that the egress policy is enforced on the address that is
//! actually connected to, not just on the string the client sent.
//!
//! Brings up a loopback TCP echo server and a wasmnet WebSocket server on the
//! shipped default policy, then asks it to reach the echo server by name.
//! `localhost` resolves into `127.0.0.0/8`, which the default policy denies, so
//! every spelling of the target has to come back `denied`.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use wasmnet::Server;
use wasmnet::policy::NetworkPolicy;

const TIMEOUT: Duration = Duration::from_secs(5);

/// Send one request and return the event that comes back.
async fn round_trip(ws_port: u16, request: serde_json::Value) -> serde_json::Value {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{ws_port}"))
        .await
        .expect("websocket connect");

    ws.send(Message::Text(request.to_string().into()))
        .await
        .unwrap();

    let read = async {
        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Text(t) = msg {
                return serde_json::from_str(&t).unwrap();
            }
        }
        panic!("connection closed without an event");
    };
    tokio::time::timeout(TIMEOUT, read)
        .await
        .expect("timed out waiting for event")
}

async fn spawn_servers() -> (u16, u16) {
    let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_port = echo.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = echo.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                while let Ok(n) = sock.read(&mut buf).await {
                    if n == 0 || sock.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_port = probe.local_addr().unwrap().port();
    drop(probe);

    let addr = SocketAddr::from(([127, 0, 0, 1], ws_port));
    tokio::spawn(async move {
        let _ = Server::new(NetworkPolicy::default(), addr).listen().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    (echo_port, ws_port)
}

#[tokio::test]
async fn hostname_cannot_reach_a_denied_range() {
    let (echo_port, ws_port) = spawn_servers().await;

    for op in ["connect", "connect_tls", "connect_udp"] {
        for addr in ["127.0.0.1", "localhost", "localhost."] {
            let ev = round_trip(
                ws_port,
                serde_json::json!({ "op": op, "id": 1, "addr": addr, "port": echo_port }),
            )
            .await;
            assert_eq!(
                ev["ev"], "denied",
                "{op} to {addr} should be denied, got {ev}"
            );
        }
    }
}

#[tokio::test]
async fn udp_send_to_is_policy_checked() {
    let (echo_port, ws_port) = spawn_servers().await;

    // Bind a UDP socket to a public address the policy allows...
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{ws_port}"))
        .await
        .unwrap();
    let bind = serde_json::json!({
        "op": "connect_udp", "id": 1, "addr": "8.8.8.8", "port": 53
    });
    ws.send(Message::Text(bind.to_string().into()))
        .await
        .unwrap();

    let ev: serde_json::Value = match ws.next().await.unwrap().unwrap() {
        Message::Text(t) => serde_json::from_str(&t).unwrap(),
        other => panic!("unexpected message: {other:?}"),
    };
    assert_eq!(ev["ev"], "udp_bound", "expected the UDP bind to succeed");

    // ...then aim a single datagram at a denied one.
    let send_to = serde_json::json!({
        "op": "send_to", "id": 1, "addr": "localhost", "port": echo_port, "data": "cGluZw=="
    });
    ws.send(Message::Text(send_to.to_string().into()))
        .await
        .unwrap();

    let ev: serde_json::Value = match ws.next().await.unwrap().unwrap() {
        Message::Text(t) => serde_json::from_str(&t).unwrap(),
        other => panic!("unexpected message: {other:?}"),
    };
    assert_eq!(ev["ev"], "denied", "send_to should be denied, got {ev}");
}
