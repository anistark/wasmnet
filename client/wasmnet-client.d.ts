export interface ListenResult {
  id: number;
  port: number;
}

export type DataCallback = (data: Uint8Array) => void;
export type CloseCallback = () => void;
export type AcceptCallback = (connId: number, remote: string) => void;

export class WasmnetClient {
  constructor(url: string);

  ready(): Promise<void>;
  connect(addr: string, port: number): Promise<number>;
  bind(addr: string, port: number): Promise<ListenResult>;
  listen(id: number, backlog?: number): void;
  send(id: number, data: string | Uint8Array | ArrayBuffer): void;
  close(id: number): void;
  onData(id: number, callback: DataCallback): void;
  onClose(id: number, callback: CloseCallback): void;
  onAccept(id: number, callback: AcceptCallback): void;
  disconnect(): void;
}
