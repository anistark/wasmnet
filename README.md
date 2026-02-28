# wasmnet

Networking proxy for browser WASM — bridges WASI socket APIs to real TCP via WebSocket.

Browser WASM cannot do raw TCP/UDP. wasmnet runs server-side and provides real network I/O on behalf of browser WASM programs, with policy controls.

```sh
Browser WASM -> WebSocket -> wasmnet server -> TCP -> Internet
```

## Quick Start

### Server

```sh
# Default policy (blocks private IPs, allows public)
cargo run

# Custom port
cargo run -- --port 8420

# Custom policy
cargo run -- --policy policy.toml

# No restrictions
cargo run -- --no-policy
```

### Browser Client

```ts
import { WasmnetClient } from './client/wasmnet-client.js';

const client = new WasmnetClient('ws://localhost:9000');
await client.ready();

const id = await client.connect('api.example.com', 443);
client.onData(id, (data) => console.log('received:', data));
client.send(id, 'GET / HTTP/1.1\r\nHost: api.example.com\r\n\r\n');
```

### Library (Embed in Rust)

```rust
use wasmnet::{Server, policy::PolicyConfig};
use std::net::SocketAddr;

let addr: SocketAddr = "0.0.0.0:9000".parse().unwrap();
let server = Server::from_config(PolicyConfig::default(), addr);
server.listen().await.unwrap();
```

## Protocol

JSON messages over a single WebSocket connection.

**Requests** (browser → server):
- `connect` — outbound TCP connection
- `bind` — bind a local port
- `listen` — start accepting connections
- `send` — send data (base64-encoded)
- `close` — close a socket

**Events** (server → browser):
- `connected`, `listening`, `accepted`, `data`, `closed`, `error`, `denied`

## Policy

Default policy blocks private IP ranges and allows all public addresses. Customize via TOML:

```toml
[network]
deny = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8"]
allow = ["*"]
bind_ports = "3000-9999"
max_connections = 50
connection_timeout_secs = 30
```

See `policy.example.toml` for a full example.
