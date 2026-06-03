//! End-to-end loopback test for the WebTransport transport.
//!
//! Brings up a TCP echo server, runs `wasmnet::webtransport::serve` against a
//! self-signed certificate, then drives a real `wtransport` client through the
//! binary protocol (`connect` → `send` → echoed `data`). Gated behind the
//! `webtransport` feature; the default `cargo test` skips it.
#![cfg(feature = "webtransport")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wasmnet::policy::Policy;
use wasmnet::proxy::SessionConfig;
use wasmnet::rate_limit::RateLimiter;
use wtransport::stream::{RecvStream, SendStream};
use wtransport::{ClientConfig, Endpoint, Identity};

// Binary protocol message types (mirrors src/binary.rs).
const CONNECT: u8 = 0x01;
const SEND: u8 = 0x04;
const CONNECTED: u8 = 0x81;
const DATA: u8 = 0x82;

const TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn webtransport_tcp_echo_roundtrip() {
    // 1. Loopback TCP echo server.
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

    // 2. Self-signed identity, and its hash for the client to trust.
    let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
    let digest = identity.certificate_chain().as_slice()[0].hash();

    // 3. Reserve an ephemeral UDP port, then serve wasmnet WebTransport on it.
    let wt_port = {
        let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let wt_addr = SocketAddr::from(([127, 0, 0, 1], wt_port));
    let config = SessionConfig {
        policy: Arc::new(Policy::allow_all()),
        rate_limiter: Arc::new(RateLimiter::unlimited()),
        pool: None,
    };
    tokio::spawn(async move {
        let _ = wasmnet::webtransport::serve(wt_addr, identity, config).await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 4. Connect a real WebTransport client and open the bidi control stream.
    let client_config = ClientConfig::builder()
        .with_bind_default()
        .with_server_certificate_hashes([digest])
        .build();
    let endpoint = Endpoint::client(client_config).unwrap();
    let connection = endpoint
        .connect(format!("https://127.0.0.1:{wt_port}"))
        .await
        .expect("connect");
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .expect("open_bi")
        .await
        .expect("stream opened");

    // 5. connect → echo server, expect a `connected` event for id 1.
    let id: u64 = 1;
    let mut connect = vec![CONNECT];
    connect.extend_from_slice(&id.to_be_bytes());
    connect.extend_from_slice(&echo_port.to_be_bytes());
    connect.extend_from_slice(b"127.0.0.1");
    write_frame(&mut send, &connect).await;

    let ev = read_frame(&mut recv).await;
    assert_eq!(ev[0], CONNECTED, "expected connected event");
    assert_eq!(u64::from_be_bytes(ev[1..9].try_into().unwrap()), id);

    // 6. send "ping", expect it echoed back as a `data` event.
    let mut data = vec![SEND];
    data.extend_from_slice(&id.to_be_bytes());
    data.extend_from_slice(b"ping");
    write_frame(&mut send, &data).await;

    let ev = read_frame(&mut recv).await;
    assert_eq!(ev[0], DATA, "expected data event");
    assert_eq!(u64::from_be_bytes(ev[1..9].try_into().unwrap()), id);
    assert_eq!(&ev[9..], b"ping", "echo payload mismatch");
}

async fn write_frame(send: &mut SendStream, body: &[u8]) {
    send.write_all(&(body.len() as u32).to_be_bytes())
        .await
        .unwrap();
    send.write_all(body).await.unwrap();
}

async fn read_frame(recv: &mut RecvStream) -> Vec<u8> {
    let read = async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.unwrap();
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await.unwrap();
        buf
    };
    tokio::time::timeout(TIMEOUT, read)
        .await
        .expect("timed out waiting for frame")
}
