export interface ListenResult {
  id: number;
  port: number;
}

export interface ClientOptions {
  /** Use binary framing instead of JSON (lower overhead for data). */
  binary?: boolean;
}

export type DataCallback = (data: Uint8Array) => void;
export type DataFromCallback = (
  data: Uint8Array,
  addr: string,
  port: number,
) => void;
export type CloseCallback = () => void;
export type AcceptCallback = (connId: number, remote: string) => void;

export class WasmnetClient {
  constructor(url: string, options?: ClientOptions);

  ready(): Promise<void>;

  /** TCP connect to a remote host. */
  connect(addr: string, port: number): Promise<number>;

  /** TCP connect with TLS (server handles TLS handshake). */
  connectTls(addr: string, port: number): Promise<number>;

  /** Create a UDP socket connected to a remote host. */
  connectUdp(addr: string, port: number): Promise<ListenResult>;

  /** Bind a TCP listener. */
  bind(addr: string, port: number): Promise<ListenResult>;

  listen(id: number, backlog?: number): void;

  /** Send data on a TCP or connected-UDP socket. */
  send(id: number, data: string | Uint8Array | ArrayBuffer): void;

  /** Send a UDP datagram to a specific address. */
  sendTo(
    id: number,
    addr: string,
    port: number,
    data: string | Uint8Array | ArrayBuffer,
  ): void;

  /** Resolve a hostname to IP addresses. */
  resolve(name: string): Promise<string[]>;

  close(id: number): void;
  onData(id: number, callback: DataCallback): void;
  onDataFrom(id: number, callback: DataFromCallback): void;
  onClose(id: number, callback: CloseCallback): void;
  onAccept(id: number, callback: AcceptCallback): void;
  disconnect(): void;
}
