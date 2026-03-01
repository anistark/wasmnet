# wasmnet

Browser client for [wasmnet](https://github.com/anistark/wasmnet) — a networking proxy that bridges WASI socket APIs to real TCP via WebSocket.

## Install

```sh
npm install wasmnet
```

## Usage

```javascript
import { WasmnetClient } from 'wasmnet';

const client = new WasmnetClient('ws://localhost:9000');
await client.ready();

// Outbound TCP connection
const id = await client.connect('api.example.com', 443);

client.onData(id, (data) => {
  console.log('received:', new TextDecoder().decode(data));
});

client.onClose(id, () => {
  console.log('connection closed');
});

client.send(id, 'GET / HTTP/1.1\r\nHost: api.example.com\r\n\r\n');

// Inbound TCP (bind a port)
const listener = await client.bind('0.0.0.0', 3000);
console.log(`listening on port ${listener.port}`);

client.onAccept(listener.id, (connId, remote) => {
  console.log(`accepted connection from ${remote}`);
  client.onData(connId, (data) => {
    client.send(connId, data); // echo
  });
});

// Cleanup
client.close(id);
client.disconnect();
```

## API

### `new WasmnetClient(url: string)`

Creates a client connecting to the wasmnet server at the given WebSocket URL.

### `ready(): Promise<void>`

Resolves when the WebSocket connection is established.

### `connect(addr: string, port: number): Promise<number>`

Opens an outbound TCP connection. Returns the socket ID.

### `bind(addr: string, port: number): Promise<{ id: number, port: number }>`

Binds a TCP listener. Returns the listener ID and actual bound port.

### `listen(id: number, backlog?: number): void`

Starts accepting connections on a bound listener.

### `send(id: number, data: string | Uint8Array | ArrayBuffer): void`

Sends data on a socket. Strings are UTF-8 encoded, binary data is sent as-is.

### `close(id: number): void`

Closes a socket or listener.

### `onData(id: number, callback: (data: Uint8Array) => void): void`

Registers a data handler. Any buffered data is flushed immediately.

### `onClose(id: number, callback: () => void): void`

Registers a close handler.

### `onAccept(id: number, callback: (connId: number, remote: string) => void): void`

Registers an accept handler for a listener.

### `disconnect(): void`

Closes the WebSocket connection and all sockets.

## MIT License
