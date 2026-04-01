const BINARY_HEADER = 9; // 1 byte type + 8 byte id

const MSG = {
  // requests
  CONNECT: 0x01,
  BIND: 0x02,
  LISTEN: 0x03,
  SEND: 0x04,
  CLOSE: 0x05,
  CONNECT_UDP: 0x06,
  SEND_TO: 0x07,
  RESOLVE: 0x08,
  CONNECT_TLS: 0x09,
  // events
  CONNECTED: 0x81,
  DATA: 0x82,
  LISTENING: 0x83,
  ACCEPTED: 0x84,
  CLOSED: 0x85,
  ERROR: 0x86,
  DENIED: 0x87,
  DATA_FROM: 0x88,
  RESOLVED: 0x89,
  UDP_BOUND: 0x8a,
};

export class WasmnetClient {
  constructor(url, options = {}) {
    this.ws = new WebSocket(url);
    this.ws.binaryType = "arraybuffer";
    this.sockets = new Map();
    this.nextId = 1;
    this.binary = options.binary === true;
    this._resolves = new Map();
    this._ready = new Promise((resolve, reject) => {
      this.ws.onopen = () => resolve();
      this.ws.onerror = (e) => reject(e);
    });
    this.ws.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        this._handleBinaryEvent(new Uint8Array(e.data));
      } else {
        this._handleEvent(JSON.parse(e.data));
      }
    };
    this.ws.onclose = () => this._handleClose();
  }

  async ready() {
    return this._ready;
  }

  connect(addr, port) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.sockets.set(id, {
        resolve,
        reject,
        buffer: [],
        onData: null,
        onClose: null,
      });
      if (this.binary) {
        this._sendBinary(MSG.CONNECT, id, this._addrPortPayload(addr, port));
      } else {
        this._send({ op: "connect", id, addr, port });
      }
    });
  }

  connectTls(addr, port) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.sockets.set(id, {
        resolve,
        reject,
        buffer: [],
        onData: null,
        onClose: null,
      });
      if (this.binary) {
        this._sendBinary(
          MSG.CONNECT_TLS,
          id,
          this._addrPortPayload(addr, port),
        );
      } else {
        this._send({ op: "connect_tls", id, addr, port });
      }
    });
  }

  connectUdp(addr, port) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.sockets.set(id, {
        resolve,
        reject,
        buffer: [],
        onData: null,
        onDataFrom: null,
        onClose: null,
      });
      if (this.binary) {
        this._sendBinary(
          MSG.CONNECT_UDP,
          id,
          this._addrPortPayload(addr, port),
        );
      } else {
        this._send({ op: "connect_udp", id, addr, port });
      }
    });
  }

  bind(addr, port) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.sockets.set(id, { resolve, reject, onAccept: null });
      if (this.binary) {
        this._sendBinary(MSG.BIND, id, this._addrPortPayload(addr, port));
      } else {
        this._send({ op: "bind", id, addr, port });
      }
    });
  }

  listen(id, backlog = 128) {
    if (this.binary) {
      const p = new Uint8Array(4);
      new DataView(p.buffer).setUint32(0, backlog);
      this._sendBinary(MSG.LISTEN, id, p);
    } else {
      this._send({ op: "listen", id, backlog });
    }
  }

  send(id, data) {
    if (this.binary) {
      const bytes =
        data instanceof Uint8Array
          ? data
          : typeof data === "string"
            ? new TextEncoder().encode(data)
            : new Uint8Array(data);
      this._sendBinary(MSG.SEND, id, bytes);
    } else {
      const encoded =
        typeof data === "string" ? btoa(data) : this._encodeBinary(data);
      this._send({ op: "send", id, data: encoded });
    }
  }

  sendTo(id, addr, port, data) {
    if (this.binary) {
      const addrBytes = new TextEncoder().encode(addr);
      const raw =
        data instanceof Uint8Array
          ? data
          : typeof data === "string"
            ? new TextEncoder().encode(data)
            : new Uint8Array(data);
      const payload = new Uint8Array(4 + addrBytes.length + raw.length);
      const dv = new DataView(payload.buffer);
      dv.setUint16(0, port);
      dv.setUint16(2, addrBytes.length);
      payload.set(addrBytes, 4);
      payload.set(raw, 4 + addrBytes.length);
      this._sendBinary(MSG.SEND_TO, id, payload);
    } else {
      const encoded =
        typeof data === "string" ? btoa(data) : this._encodeBinary(data);
      this._send({ op: "send_to", id, addr, port, data: encoded });
    }
  }

  resolve(name) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this._resolves.set(id, { resolve, reject });
      if (this.binary) {
        this._sendBinary(MSG.RESOLVE, id, new TextEncoder().encode(name));
      } else {
        this._send({ op: "resolve", id, name });
      }
    });
  }

  close(id) {
    if (this.binary) {
      this._sendBinary(MSG.CLOSE, id, new Uint8Array(0));
    } else {
      this._send({ op: "close", id });
    }
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

  onDataFrom(id, callback) {
    const sock = this.sockets.get(id);
    if (sock) sock.onDataFrom = callback;
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

  // ── JSON transport ───────────────────────────────────

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

  // ── Binary transport ─────────────────────────────────

  _sendBinary(type_, id, payload) {
    const frame = new Uint8Array(BINARY_HEADER + payload.length);
    frame[0] = type_;
    const dv = new DataView(frame.buffer);
    dv.setBigUint64(1, BigInt(id));
    frame.set(payload, BINARY_HEADER);
    this.ws.send(frame.buffer);
  }

  _addrPortPayload(addr, port) {
    const addrBytes = new TextEncoder().encode(addr);
    const payload = new Uint8Array(2 + addrBytes.length);
    new DataView(payload.buffer).setUint16(0, port);
    payload.set(addrBytes, 2);
    return payload;
  }

  _handleBinaryEvent(data) {
    if (data.length < BINARY_HEADER) return;
    const type_ = data[0];
    const dv = new DataView(data.buffer, data.byteOffset, data.byteLength);
    const id = Number(dv.getBigUint64(1));
    const payload = data.subarray(BINARY_HEADER);

    switch (type_) {
      case MSG.CONNECTED:
        this._dispatch(id, "connected", {});
        break;
      case MSG.DATA:
        this._dispatch(id, "data", { raw: payload });
        break;
      case MSG.LISTENING:
        this._dispatch(id, "listening", {
          port: new DataView(
            payload.buffer,
            payload.byteOffset,
            payload.byteLength,
          ).getUint16(0),
        });
        break;
      case MSG.ACCEPTED: {
        const connId = Number(
          new DataView(
            payload.buffer,
            payload.byteOffset,
            payload.byteLength,
          ).getBigUint64(0),
        );
        const remote = new TextDecoder().decode(payload.subarray(8));
        this._dispatch(id, "accepted", { conn_id: connId, remote });
        break;
      }
      case MSG.CLOSED:
        this._dispatch(id, "closed", {});
        break;
      case MSG.ERROR:
        this._dispatch(id, "error", {
          msg: new TextDecoder().decode(payload),
        });
        break;
      case MSG.DENIED:
        this._dispatch(id, "denied", {
          msg: new TextDecoder().decode(payload),
        });
        break;
      case MSG.DATA_FROM: {
        const pdv = new DataView(
          payload.buffer,
          payload.byteOffset,
          payload.byteLength,
        );
        const port = pdv.getUint16(0);
        const addrLen = pdv.getUint16(2);
        const addr = new TextDecoder().decode(payload.subarray(4, 4 + addrLen));
        const raw = payload.subarray(4 + addrLen);
        this._dispatch(id, "data_from", { addr, port, raw });
        break;
      }
      case MSG.RESOLVED: {
        const addrs = JSON.parse(new TextDecoder().decode(payload));
        this._dispatchResolve(id, addrs);
        break;
      }
      case MSG.UDP_BOUND:
        this._dispatch(id, "udp_bound", {
          port: new DataView(
            payload.buffer,
            payload.byteOffset,
            payload.byteLength,
          ).getUint16(0),
        });
        break;
    }
  }

  _dispatch(id, ev, extra) {
    const sock = this.sockets.get(id);
    if (!sock && ev !== "resolved") return;

    switch (ev) {
      case "connected":
        sock.resolve?.(id);
        break;
      case "listening":
        sock.resolve?.({ id, port: extra.port });
        break;
      case "udp_bound":
        sock.resolve?.({ id, port: extra.port });
        break;
      case "data": {
        const decoded = extra.raw || this._decodeBinary(extra.b64);
        if (sock.onData) {
          sock.onData(decoded);
        } else {
          sock.buffer.push(decoded);
        }
        break;
      }
      case "data_from":
        sock.onDataFrom?.(extra.raw, extra.addr, extra.port);
        break;
      case "accepted":
        this.sockets.set(extra.conn_id, {
          buffer: [],
          onData: null,
          onClose: null,
        });
        sock.onAccept?.(extra.conn_id, extra.remote);
        break;
      case "closed":
        sock.onClose?.();
        this.sockets.delete(id);
        break;
      case "error":
        sock.reject?.(new Error(extra.msg));
        this.sockets.delete(id);
        break;
      case "denied":
        sock.reject?.(new Error(extra.msg));
        this.sockets.delete(id);
        break;
    }
  }

  _dispatchResolve(id, addrs) {
    const entry = this._resolves.get(id);
    if (entry) {
      entry.resolve(addrs);
      this._resolves.delete(id);
    }
  }

  // ── JSON event handler ───────────────────────────────

  _handleEvent(ev) {
    if (ev.ev === "resolved") {
      this._dispatchResolve(ev.id, ev.addrs);
      return;
    }

    const sock = this.sockets.get(ev.id);
    if (!sock) return;

    switch (ev.ev) {
      case "connected":
        sock.resolve(ev.id);
        break;
      case "listening":
        sock.resolve({ id: ev.id, port: ev.port });
        break;
      case "udp_bound":
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
      case "data_from": {
        const decoded = this._decodeBinary(ev.data);
        sock.onDataFrom?.(decoded, ev.addr, ev.port);
        break;
      }
      case "accepted":
        this.sockets.set(ev.conn_id, {
          buffer: [],
          onData: null,
          onClose: null,
        });
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
    for (const [, sock] of this.sockets) {
      sock.onClose?.();
    }
    this.sockets.clear();
    for (const [, entry] of this._resolves) {
      entry.reject?.(new Error("connection closed"));
    }
    this._resolves.clear();
  }
}
