# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-04-01

### Added

- **TLS termination** — `connect_tls` request performs server-side TLS handshake via rustls/webpki-roots and proxies decrypted data (`src/proxy.rs`)
- **UDP support** — `connect_udp` / `send` / `recv_from` with per-socket cancel (`src/proxy.rs`)
- **DNS resolution** — `resolve` request returns IP addresses for a hostname (`src/dns.rs`)
- **Binary framing** — `[1B type][8B id][payload]` over WebSocket binary messages, auto-detected per session (`src/binary.rs`)
- **Bandwidth rate limiting** — token-bucket limiter enforcing `max_bandwidth_mbps` policy on all data paths (`src/rate_limit.rs`)
- **Connection pooling** — idle TCP pool with configurable TTL and background cleanup (`src/pool.rs`)
- `ConnectTls`, `ConnectUdp`, `Resolve` request variants and `Resolved` event in protocol
- `connectTls()`, `connectUdp()`, `resolve()` methods in browser client
- Binary framing support in browser client (auto-negotiated)
- TypeScript declarations for all new client methods
- MIT license (`LICENSE`)
- 15 unit tests (policy, rate limiting, DNS, binary framing, connection pooling)

### Changed

- Refactored proxy session handlers to use `SessionCtx` struct
- `max_bandwidth_mbps` policy field is now enforced (previously parsed but ignored)
- Updated README and client README with full Phase 4 feature documentation

## [0.1.1] - 2026-03-01

### Added

- Browser client package (`client/`) with ES module source and TypeScript declarations
- `npm install wasmnet` support via `client/package.json`
- Client README with API documentation

### Fixed

- Package naming for npm distribution

## [0.1.0] - 2026-03-01

### Added

- Outbound TCP proxy — `connect`, `send`, `close` requests with bidirectional WebSocket ↔ TCP bridge
- Inbound TCP proxy — `bind`, `listen`, `accept` for port export
- JSON protocol over WebSocket with base64-encoded data payloads
- Per-session socket tracking by numeric ID
- Connection timeout support
- Policy engine with allow/deny lists for IP addresses (CIDR) and domains (exact + wildcard)
- Deny-by-default mode with explicit allow list
- Port binding restrictions (range and individual syntax)
- Connection count limits (`max_connections`)
- Default safe policy blocking private IP ranges (`10/8`, `172.16/12`, `192.168/16`, `127/8`, `169.254/16`)
- Policy configuration from TOML files
- Standalone binary: `wasmnet-server` with clap CLI (`--port`, `--policy`, `--no-policy`)
- Library API: `Server::builder()`, `Server::new()`, `Server::from_config()`, `Server::allow_all()`
- Embedding support: `handle_ws_upgrade()` for integrating into existing servers
- Graceful shutdown via `listen_with_shutdown()` with oneshot channel
- `load_policy_file()` helper
- Structured logging with `tracing` (env-filter via `RUST_LOG`)
- 4 policy engine unit tests

[Unreleased]: https://github.com/anistark/wasmnet/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/anistark/wasmnet/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/anistark/wasmnet/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/anistark/wasmnet/releases/tag/v0.1.0
