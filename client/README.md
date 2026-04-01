# wasmnet

Browser client for [wasmnet](https://github.com/anistark/wasmnet) — a networking proxy that bridges WASI socket APIs to real TCP/UDP/TLS via WebSocket.

## Install

```sh
npm install wasmnet
```

## Usage

```javascript
import { WasmnetClient } from 'wasmnet';

const client = new WasmnetClient('ws://localhost:9000');
await client.ready();

// TCP
const id = await client.connect('example.com', 80);
client.onData(id, (data) => console.log(new TextDecoder().decode(data)));
client.send(id, 'GET / HTTP/1.1\r\nHost: example.com\r\n\r\n');

// TLS (server handles handshake)
const tls = await client.connectTls('api.example.com', 443);
client.onData(tls, (data) => console.log('tls:', data));

// UDP
const udp = await client.connectUdp('8.8.8.8', 53);
client.onDataFrom(udp.id, (data, addr, port) => console.log(data));
client.send(udp.id, packet);
client.sendTo(udp.id, '8.8.4.4', 53, packet); // different target

// DNS resolve
const ips = await client.resolve('example.com');

// Inbound TCP
const listener = await client.bind('0.0.0.0', 3000);
client.onAccept(listener.id, (connId, remote) => {
  client.onData(connId, (data) => client.send(connId, data));
});

// Cleanup
client.close(id);
client.disconnect();
```

### Binary framing

Pass `{ binary: true }` to skip JSON + base64 overhead on data frames. Raw bytes are sent directly in WebSocket binary messages.

```javascript
const client = new WasmnetClient('ws://localhost:9000', { binary: true });
```

## API

### `new WasmnetClient(url: string, options?: { binary?: boolean })`

Creates a client connecting to the wasmnet server. Set `binary: true` for binary framing mode.

### `ready(): Promise<void>`

Resolves when the WebSocket connection is open.

### `connect(addr: string, port: number): Promise<number>`

Opens an outbound TCP connection. Returns the socket ID.

### `connectTls(addr: string, port: number): Promise<number>`

Opens an outbound TCP connection with TLS. The wasmnet server handles the TLS handshake (using system CA roots). Returns the socket ID. Data sent/received through this socket is plaintext — encryption is handled transparently.

### `connectUdp(addr: string, port: number): Promise<{ id: number, port: number }>`

Creates a UDP socket connected to the given address. Returns the socket ID and local port. Use `send()` to send to the connected address, or `sendTo()` for arbitrary targets.

### `bind(addr: string, port: number): Promise<{ id: number, port: number }>`

Binds a TCP listener. Returns the listener ID and actual bound port.

### `listen(id: number, backlog?: number): void`

Starts accepting connections on a bound listener.

### `send(id: number, data: string | Uint8Array | ArrayBuffer): void`

Sends data on a TCP, TLS, or connected UDP socket.

### `sendTo(id: number, addr: string, port: number, data: string | Uint8Array | ArrayBuffer): void`

Sends a UDP datagram to a specific target address, regardless of the socket's connected address.

### `resolve(name: string): Promise<string[]>`

Resolves a hostname to an array of IP address strings (both IPv4 and IPv6).

### `close(id: number): void`

Closes a socket or listener.

### `onData(id: number, callback: (data: Uint8Array) => void): void`

Registers a data handler for TCP/TLS sockets. Any data buffered before the handler was set is flushed immediately.

### `onDataFrom(id: number, callback: (data: Uint8Array, addr: string, port: number) => void): void`

Registers a data handler for UDP sockets. Each callback includes the source address and port.

### `onClose(id: number, callback: () => void): void`

Registers a close handler.

### `onAccept(id: number, callback: (connId: number, remote: string) => void): void`

Registers an accept handler for a TCP listener. `connId` is the new socket ID for the accepted connection.

### `disconnect(): void`

Closes the WebSocket connection and all sockets.

## License

MIT
