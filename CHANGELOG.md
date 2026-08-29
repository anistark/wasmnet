# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Hostname targets no longer bypass the IP deny list** (#3) — a name is now resolved before the policy decides, and every resolved address is checked against the CIDR rules. The connection is made to the address that was approved rather than by re-resolving the name, so a second lookup cannot substitute a different one. Applies to `connect`, `connect_tls` and `connect_udp`; TLS keeps the original hostname for SNI. Previously `localhost` and `metadata.google.internal` were allowed under the default policy that blocks `127.0.0.0/8` and `169.254.0.0/16` (`src/policy.rs`, `src/proxy.rs`)
- **UDP `send_to` is now policy-checked** — each datagram carries its own destination, and that destination was never checked, so a socket bound to an allowed target could send anywhere (`src/proxy.rs`)
- **Ports in allow/deny rules are enforced** (#4) — a rule like `api.example.com:443` no longer applies to every port on the host. Ports also work on CIDR and bare-IP rules (`10.0.0.0/8:22`, `[::1]:80`); a rule with no port still covers every port. Removed the dead `allow_nets` string comparison in `is_allowed_domain` (`src/policy.rs`)
- **Domain rules cover subdomains, the apex, and the trailing-dot form** (#5) — `example.com` and `*.example.com` are now equivalent and both match subdomains, and a single trailing dot is stripped from rules and targets, so `evil.com.` can no longer opt out of a deny rule written as `evil.com` (`src/policy.rs`)

### Changed

- Bare IP addresses in a rule (`1.2.3.4`, `::1`) are parsed as single-host networks instead of falling through to domain matching, so IPv6 literals are handled correctly
- Allow rules are now more permissive for subdomains: `allow = ["example.com"]` covers `sub.example.com`, where before it matched only the exact name. Narrow an allow rule with a port if that matters (`example.com:443`)
- `Policy::check_resolved` is new public API, for embedders enforcing the same rules on addresses they resolved themselves

## [0.1.4] - 2026-06-02

### Added

- **WebTransport transport** (opt-in `webtransport` cargo feature) — serves the existing request/event protocol over HTTP/3 / QUIC alongside WebSocket, for lower latency. Length-prefixed binary frames over one bidirectional stream, reusing the `binary` codec. Server supports a PEM `--cert`/`--key` pair or a generated self-signed dev certificate (SHA-256 logged for the browser's `serverCertificateHashes`). New `wasmnet-server --webtransport-port <p>` flag and `Server::listen_webtransport()` API (`src/webtransport.rs`)
- Browser client `transport: 'webtransport'` option (with `serverCertificateHashes`) — same API over WebTransport, always binary-framed (`client/wasmnet-client.js`)
- Loopback WebTransport integration test driving a real `wtransport` client through connect → send → echo (`tests/webtransport.rs`)

### Changed

- Refactored proxy session handling to a transport-agnostic core (`dispatch` / `cleanup` / `Ctx::new`) shared by the WebSocket and WebTransport listeners
- Browser client transport is now pluggable (`WebSocketTransport` / `WebTransportTransport`); WebSocket behavior unchanged

## [0.1.3] - 2026-04-04

> Published as 0.1.3 — the 0.1.2 version was never released (skipped due to a partial publish), so these changes shipped in the 0.1.3 crate.

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

[Unreleased]: https://github.com/anistark/wasmnet/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/anistark/wasmnet/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/anistark/wasmnet/compare/v0.1.1...v0.1.3
[0.1.1]: https://github.com/anistark/wasmnet/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/anistark/wasmnet/releases/tag/v0.1.0
