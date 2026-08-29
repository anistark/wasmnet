# wasmnet

[![Crates.io](https://img.shields.io/crates/v/wasmnet)](https://crates.io/crates/wasmnet)
[![docs.rs](https://img.shields.io/docsrs/wasmnet)](https://docs.rs/wasmnet)
[![Crates.io Downloads](https://img.shields.io/crates/d/wasmnet)](https://crates.io/crates/wasmnet)
[![License: MIT](https://img.shields.io/crates/l/wasmnet)](LICENSE)

Networking proxy for browser WASM — bridges WASI socket APIs to real TCP/UDP via WebSocket.

Browser WASM cannot do raw TCP or UDP. wasmnet runs server-side and provides real network I/O on behalf of browser WASM programs, with policy controls and bandwidth limiting.

```
Browser WASM ──WebSocket──▶ wasmnet server ──TCP/UDP/TLS──▶ Internet
```

## Features

- **TCP proxy** — outbound `connect` and inbound `bind`/`listen`/`accept`
- **TLS termination** — `connect_tls` handles the TLS handshake server-side (rustls + webpki-roots)
- **UDP support** — `connect_udp` / `send` / `send_to` with async receive
- **DNS resolution** — `resolve` a hostname to IP addresses without opening a socket
- **Policy engine** — allow/deny lists for IPs (CIDR), domains and their subdomains, per-rule ports, port ranges, connection limits. Hostnames are re-checked against the CIDR rules once resolved, so a name cannot reach a denied range
- **Bandwidth limiting** — token-bucket rate limiter enforcing `max_bandwidth_mbps`
- **Connection pooling** — reuse idle TCP connections with configurable TTL and warm-up
- **Binary framing** — optional `[1B type][8B id][payload]` binary protocol, auto-detected alongside JSON
- **WebTransport** — optional HTTP/3 / QUIC transport (opt-in `webtransport` feature) carrying the same protocol with lower latency than WebSocket
- **Embeddable** — use as a library with `Server::builder()` or upgrade a single TCP stream with `handle_ws_upgrade()`

## Install

### Server (Rust)

```bash
cargo install wasmnet
```

### Browser Client (npm)

```bash
npm install wasmnet
```

## Quick Start

### Run the server

```bash
# Default policy (blocks private IPs, allows public)
wasmnet-server

# Custom port
wasmnet-server --port 8420

# With a policy file
wasmnet-server --policy policy.toml

# No restrictions
wasmnet-server --no-policy

# Bandwidth limit of 5 Mbps + connection pooling
wasmnet-server --max-bandwidth-mbps 5 --pool-idle-secs 60 --pool-per-key 4
```

#### WebTransport (HTTP/3)

Build with the `webtransport` feature to also serve the protocol over QUIC on a UDP port, alongside WebSocket:

```bash
cargo install wasmnet --features webtransport

# Self-signed dev certificate (the SHA-256 is logged at startup)
wasmnet-server --webtransport-port 9001

# Production: supply a CA-trusted PEM keypair
wasmnet-server --webtransport-port 9001 --cert cert.pem --key key.pem
```

### Browser client

[![npm](https://img.shields.io/npm/v/wasmnet)](https://www.npmjs.com/package/wasmnet)
[![npm downloads](https://img.shields.io/npm/dm/wasmnet)](https://www.npmjs.com/package/wasmnet)

```javascript
import { WasmnetClient } from 'wasmnet';

const client = new WasmnetClient('ws://localhost:9000');
await client.ready();

// ── TCP ────────────────────────────────────
const id = await client.connect('example.com', 80);
client.onData(id, (data) => console.log(new TextDecoder().decode(data)));
client.send(id, 'GET / HTTP/1.1\r\nHost: example.com\r\n\r\n');

// ── TLS ────────────────────────────────────
const tls = await client.connectTls('api.example.com', 443);
client.onData(tls, (data) => console.log('tls:', data));
client.send(tls, 'GET / HTTP/1.1\r\nHost: api.example.com\r\n\r\n');

// ── UDP ────────────────────────────────────
const udp = await client.connectUdp('8.8.8.8', 53);
client.onDataFrom(udp.id, (data, addr, port) => {
  console.log(`dns reply from ${addr}:${port}`, data);
});
client.send(udp.id, dnsQueryPacket);

// ── DNS resolve ────────────────────────────
const ips = await client.resolve('example.com');
console.log(ips); // ["93.184.216.34", "2606:2800:220:1:..."]

// ── Inbound TCP (port binding) ─────────────
const listener = await client.bind('0.0.0.0', 3000);
console.log(`listening on port ${listener.port}`);
client.onAccept(listener.id, (connId, remote) => {
  console.log(`connection from ${remote}`);
  client.onData(connId, (data) => client.send(connId, data)); // echo
});

// ── Cleanup ────────────────────────────────
client.close(id);
client.disconnect();
```

#### Binary framing mode

Pass `{ binary: true }` to avoid JSON + base64 overhead on data frames:

```javascript
const client = new WasmnetClient('ws://localhost:9000', { binary: true });
```

The server auto-detects the framing per message — JSON text frames and binary frames can even be mixed within the same session.

#### WebTransport mode

Pass `{ transport: 'webtransport' }` to use HTTP/3 / QUIC instead of WebSocket (same API, always binary-framed). Requires a server started with `--webtransport-port`:

```javascript
const client = new WasmnetClient('https://localhost:9001', {
  transport: 'webtransport',
  // For a self-signed dev cert, pass the SHA-256 the server logs at startup:
  // serverCertificateHashes: [{ algorithm: 'sha-256', value: hashBytes }],
});
await client.ready();
```

### Library (embed in Rust)

```rust
use wasmnet::Server;

// Builder API
let server = Server::builder()
    .host("0.0.0.0")
    .port(9000)
    .policy_file("policy.toml")?
    .max_bandwidth_mbps(10)
    .pool(60, 8)        // idle 60s, 8 per target
    .build()?;
server.listen().await?;
```

```rust
// Graceful shutdown
let (tx, rx) = tokio::sync::oneshot::channel();
let server = Server::builder().no_policy().build()?;
server.listen_with_shutdown(rx).await?;
```

```rust
// Embed in an existing server — upgrade a single TCP stream
use wasmnet::{handle_ws_upgrade, policy::Policy};
use std::sync::Arc;

let policy = Arc::new(Policy::allow_all());
handle_ws_upgrade(tcp_stream, policy).await;
```

## CLI Reference

```
wasmnet-server [OPTIONS]

Options:
  -H, --host <HOST>                   Listen address [default: 0.0.0.0]
  -p, --port <PORT>                   Listen port [default: 9000]
      --policy <FILE>                 Path to policy TOML file
      --no-policy                     Disable all policy checks
      --max-bandwidth-mbps <MBPS>     Bandwidth limit (overrides policy file)
      --pool-idle-secs <SECS>         Enable connection pooling with idle timeout
      --pool-per-key <N>              Max pooled connections per target [default: 8]

  # Requires building with --features webtransport:
      --webtransport-port <PORT>      Also serve WebTransport (HTTP/3) on this UDP port
      --cert <FILE>                   TLS certificate PEM (else self-signed dev cert)
      --key <FILE>                    TLS private key PEM
```

Set `RUST_LOG=wasmnet=debug` for detailed logging.

## Protocol

Single WebSocket connection. Messages are JSON text frames or binary frames (auto-detected).

### Requests (browser → server)

| Operation | Fields | Description |
|---|---|---|
| `connect` | `id`, `addr`, `port` | Outbound TCP connection |
| `connect_tls` | `id`, `addr`, `port` | Outbound TCP + TLS (server-side handshake) |
| `connect_udp` | `id`, `addr`, `port` | Create a UDP socket connected to target |
| `bind` | `id`, `addr`, `port` | Bind a local TCP listener |
| `listen` | `id`, `backlog?` | Start accepting connections |
| `send` | `id`, `data` (base64) | Send data on a TCP or UDP socket |
| `send_to` | `id`, `addr`, `port`, `data` (base64) | Send a UDP datagram to a specific address |
| `resolve` | `id`, `name` | DNS lookup — resolve hostname to IPs |
| `close` | `id` | Close a socket or listener |

### Events (server → browser)

| Event | Fields | Description |
|---|---|---|
| `connected` | `id` | TCP or TLS connection established |
| `data` | `id`, `data` (base64) | Data received on TCP/TLS socket |
| `data_from` | `id`, `data`, `addr`, `port` | UDP datagram received |
| `udp_bound` | `id`, `port` | UDP socket bound |
| `listening` | `id`, `port` | TCP listener bound |
| `accepted` | `id`, `conn_id`, `remote` | New inbound TCP connection |
| `resolved` | `id`, `addrs` | DNS result (array of IP strings) |
| `closed` | `id` | Socket closed |
| `error` | `id`, `msg` | Error occurred |
| `denied` | `id`, `msg` | Blocked by policy |

### Binary frame format

When the client sends a WebSocket binary message, the server switches to binary framing for responses. Frame layout:

```
[1 byte message type][8 bytes connection ID (big-endian)][payload…]
```

Data payloads are raw bytes — no base64 encoding. See `src/binary.rs` for type constants.

## Policy

Configure via TOML. The default policy blocks private IP ranges and allows all public addresses.

```toml
[network]
# Block private/internal ranges
deny = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8", "169.254.0.0/16"]

# Allow everything else
allow = ["*"]

# Port binding restricted to user range
bind_ports = "3000-9999"

# Limits
max_connections = 50
max_bandwidth_mbps = 10
connection_timeout_secs = 30
```

### Deny-by-default mode

```toml
[network]
deny = ["*"]
allow = ["api.example.com:443", "*.github.com:443"]
bind_ports = "3000,8080"
max_connections = 5
```

Features:
- **CIDR matching** — `10.0.0.0/8`, `172.16.0.0/12`, or a bare IP (`1.2.3.4`, `::1`)
- **Domain patterns** — `example.com` and `*.example.com` both cover the domain and every subdomain of it
- **Port-qualified rules** — append a port to any rule (`api.example.com:443`, `10.0.0.0/8:22`, `[::1]:80`); without one the rule covers every port
- **Post-resolution enforcement** — a hostname is checked again against the CIDR rules once resolved, so `localhost` cannot reach a denied `127.0.0.1`
- **Port ranges** — `3000-9999` or `3000,8080,9090`
- **Connection limits** — `max_connections` per session
- **Bandwidth limiting** — `max_bandwidth_mbps` enforced via token-bucket rate limiter
- **Connection timeout** — `connection_timeout_secs` for outbound TCP connects

#### Rule matching

- **Deny wins.** Deny rules are evaluated first; a target that matches one is refused regardless of the allow list.
- **Names are normalized.** Rules and targets are compared case-folded and with any single trailing dot removed, so `evil.com`, `EVIL.com` and `evil.com.` are one name.
- **Domain rules cover subdomains.** `example.com` and `*.example.com` are equivalent: both match `example.com` and `sub.example.com`. Suffix lookalikes do not match — `evil.com` does not cover `notevil.com`.
- **Hostnames are checked twice.** First against the domain rules as written, then once per resolved address against the CIDR rules. Every resolved address must clear the deny list, and the connection is made to the address that was approved rather than by re-resolving the name, so a second lookup cannot substitute a different answer.
- **UDP datagrams carry their own destination**, so each `send_to` is checked, not only the initial bind.

See [`policy.example.toml`](policy.example.toml) for a full example.

## Architecture

```
┌─ Browser WASM ──────────────────────────────────────────────┐
│  WASI socket call → serialize → WebSocket.send()            │
└──────────────────────────┬──────────────────────────────────┘
                           │ WebSocket (JSON or binary frames)
┌──────────────────────────▼──────────────────────────────────┐
│  wasmnet server                                              │
│                                                              │
│  ┌──────────┐  ┌─────────────┐  ┌───────────────────┐       │
│  │  Policy   │  │ Rate Limiter│  │ Connection Pool   │       │
│  │  Engine   │  │ (token      │  │ (idle TCP streams  │       │
│  │          │  │  bucket)    │  │  w/ TTL + warmup) │       │
│  └────┬─────┘  └──────┬──────┘  └────────┬──────────┘       │
│       │               │                  │                   │
│       ▼               ▼                  ▼                   │
│  ┌─────────────────────────────────────────────┐             │
│  │  Proxy: bidirectional bridge                 │             │
│  │  WebSocket frames ↔ TCP / TLS / UDP bytes    │             │
│  └─────────────────────────────────────────────┘             │
└──────────────────────────────────────────────────────────────┘
         │            │            │
         ▼            ▼            ▼
     TCP stream   TLS stream   UDP socket
```

### Source layout

```
src/
├── main.rs        # CLI binary (clap)
├── lib.rs         # Server, ServerBuilder, handle_ws_upgrade
├── proxy.rs       # WebSocket ↔ TCP/TLS/UDP bidirectional proxy
├── policy.rs      # Allow/deny lists, CIDR, domains, rate limits
├── protocol.rs    # Request/Event message types (serde JSON)
├── binary.rs      # Binary frame codec
├── rate_limit.rs  # Token-bucket bandwidth limiter
├── dns.rs         # Async DNS resolution
├── pool.rs        # Idle TCP connection pool
└── webtransport.rs # WebTransport (HTTP/3) transport — `webtransport` feature

client/
├── wasmnet-client.js    # ES module (WebSocket / WebTransport, JSON + binary framing)
├── wasmnet-client.d.ts  # TypeScript declarations
└── README.md
```

### [Open Source MIT License](./LICENSE)
