export class WasmnetClient {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.sockets = new Map();
    this.nextId = 1;
    this._ready = new Promise((resolve, reject) => {
      this.ws.onopen = () => resolve();
      this.ws.onerror = (e) => reject(e);
    });
    this.ws.onmessage = (e) => this._handleEvent(JSON.parse(e.data));
    this.ws.onclose = () => this._handleClose();
  }

  async ready() {
    return this._ready;
  }

  connect(addr, port) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.sockets.set(id, { resolve, reject, buffer: [], onData: null, onClose: null });
      this._send({ op: "connect", id, addr, port });
    });
  }

  bind(addr, port) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.sockets.set(id, { resolve, reject, onAccept: null });
      this._send({ op: "bind", id, addr, port });
    });
  }

  listen(id, backlog = 128) {
    this._send({ op: "listen", id, backlog });
  }

  send(id, data) {
    const encoded = typeof data === "string" ? btoa(data) : this._encodeBinary(data);
    this._send({ op: "send", id, data: encoded });
  }

  close(id) {
    this._send({ op: "close", id });
    this.sockets.delete(id);
  }

  onData(id, callback) {
    const sock = this.sockets.get(id);
    if (!sock) return;
    sock.onData = callback;
    for (const buffered of sock.buffer) {
      callback(buffered);
    }
    sock.buffer = [];
  }

  onClose(id, callback) {
    const sock = this.sockets.get(id);
    if (sock) sock.onClose = callback;
  }

  onAccept(id, callback) {
    const sock = this.sockets.get(id);
    if (sock) sock.onAccept = callback;
  }

  disconnect() {
    this.ws.close();
  }

  _send(msg) {
    this.ws.send(JSON.stringify(msg));
  }

  _encodeBinary(data) {
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    let binary = "";
    for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
  }

  _decodeBinary(b64) {
    const binary = atob(b64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }

  _handleEvent(ev) {
    const sock = this.sockets.get(ev.id);
    if (!sock) return;

    switch (ev.ev) {
      case "connected":
        sock.resolve(ev.id);
        break;
      case "listening":
        sock.resolve({ id: ev.id, port: ev.port });
        break;
      case "data": {
        const decoded = this._decodeBinary(ev.data);
        if (sock.onData) {
          sock.onData(decoded);
        } else {
          sock.buffer.push(decoded);
        }
        break;
      }
      case "accepted":
        this.sockets.set(ev.conn_id, { buffer: [], onData: null, onClose: null });
        sock.onAccept?.(ev.conn_id, ev.remote);
        break;
      case "closed":
        sock.onClose?.();
        this.sockets.delete(ev.id);
        break;
      case "error":
        sock.reject?.(new Error(ev.msg));
        this.sockets.delete(ev.id);
        break;
      case "denied":
        sock.reject?.(new Error(ev.msg));
        this.sockets.delete(ev.id);
        break;
    }
  }

  _handleClose() {
    for (const [id, sock] of this.sockets) {
      sock.onClose?.();
    }
    this.sockets.clear();
  }
}
