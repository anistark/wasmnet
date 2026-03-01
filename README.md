# wasmnet

Networking proxy for browser WASM — bridges WASI socket APIs to real TCP via WebSocket.

Browser WASM cannot do raw TCP/UDP. wasmnet runs server-side and provides real network I/O on behalf of browser WASM programs, with policy controls.

```
Browser WASM ──WebSocket──► wasmnet server ──TCP──► Internet
```

## Install

### Server (Rust)

```bash
cargo install wasmnet
```

### Browser Client (npm)

```bash
npm install @aspect-run/wasmnet-client
```

## Quick Start

### Standalone Server

```bash
# Default policy (blocks private IPs, allows public)
wasmnet-server

# Custom port
wasmnet-server --port 8420

# Custom policy file
wasmnet-server --policy policy.toml

# No restrictions
wasmnet-server --no-policy
```

### Browser Client

```javascript
import { WasmnetClient } from '@aspect-run/wasmnet-client';

const client = new WasmnetClient('ws://localhost:9000');
await client.ready();

// Outbound TCP
const id = await client.connect('api.example.com', 443);
client.onData(id, (data) => console.log('received:', data));
client.send(id, 'GET / HTTP/1.1\r\nHost: api.example.com\r\n\r\n');

// Inbound TCP (bind a port)
const listener = await client.bind('0.0.0.0', 3000);
client.onAccept(listener.id, (connId, remote) => {
  client.onData(connId, (data) => client.send(connId, data));
});
```

### Library (Embed in Rust)

```rust
use wasmnet::Server;

// Builder API
let server = Server::builder()
    .host("0.0.0.0")
    .port(9000)
    .policy_file("policy.toml")?
    .build()?;
server.listen().await?;

// Or with graceful shutdown
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
let server = Server::builder().no_policy().build()?;
server.listen_with_shutdown(shutdown_rx).await?;

// Direct construction
use wasmnet::policy::PolicyConfig;
let addr = "0.0.0.0:9000".parse().unwrap();
let server = Server::from_config(PolicyConfig::default(), addr);

// Upgrade a single TCP stream (for embedding in existing servers)
use wasmnet::{handle_ws_upgrade, policy::Policy};
use std::sync::Arc;
let policy = Arc::new(Policy::allow_all());
handle_ws_upgrade(tcp_stream, policy).await;
```

## Protocol

JSON messages over a single WebSocket connection.

### Requests (browser → server)

| Operation | Fields | Description |
|-----------|--------|-------------|
| `connect` | `id`, `addr`, `port` | Outbound TCP connection |
| `bind` | `id`, `addr`, `port` | Bind a local TCP port |
| `listen` | `id`, `backlog?` | Start accepting connections |
| `send` | `id`, `data` (base64) | Send data on a socket |
| `close` | `id` | Close a socket or listener |

### Events (server → browser)

| Event | Fields | Description |
|-------|--------|-------------|
| `connected` | `id` | Connection established |
| `data` | `id`, `data` (base64) | Data received |
| `listening` | `id`, `port` | Listener bound |
| `accepted` | `id`, `conn_id`, `remote` | New inbound connection |
| `closed` | `id` | Socket closed |
| `error` | `id`, `msg` | Error occurred |
| `denied` | `id`, `msg` | Blocked by policy |

## Policy

Default policy blocks private IP ranges and allows all public addresses. Customize via TOML:

```toml
[network]
deny = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8", "169.254.0.0/16"]
allow = ["*"]
bind_ports = "3000-9999"
max_connections = 50
connection_timeout_secs = 30
```

### Deny-by-default mode

```toml
[network]
deny = ["*"]
allow = ["api.example.com:443", "*.github.com:443"]
max_connections = 5
```

See [`policy.example.toml`](policy.example.toml) for a full example.

## Architecture

```
┌─ Browser WASM ──────────────────────────────────┐
│  WASI socket call → serialize → WebSocket.send  │
└──────────────────────┬──────────────────────────┘
                       │ WebSocket
┌──────────────────────▼──────────────────────────┐
│  wasmnet server                                  │
│  1. Policy check (allow/deny, rate limits)       │
│  2. Real TCP connect/bind                        │
│  3. Bidirectional proxy: WS frames ↔ TCP bytes   │
└─────────────────────────────────────────────────┘
```

## License

MIT
