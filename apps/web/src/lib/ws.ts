import type { ClientMessage, ServerMessage } from "@lattice/shared-types";

import { GATEWAY_URL } from "@/lib/gateway";

const WS_URL = `${GATEWAY_URL.replace(/^http/, "ws")}/ws`;

/**
 * The browser's data socket to the gateway. Phase 0 uses it for control messages only
 * (hello, ping/pong); phase 1 adds per-sheet query batches answered with Arrow IPC frames.
 */
export class GatewaySocket {
  private socket: WebSocket | null = null;
  private nextPingId = 1;
  private closedByUs = false;

  constructor(
    private readonly onMessage: (message: ServerMessage) => void,
    private readonly onStatus: (status: "connecting" | "open" | "closed") => void = () => {},
  ) {}

  connect(): void {
    if (this.socket) return;
    this.closedByUs = false;
    this.onStatus("connecting");
    const socket = new WebSocket(WS_URL);
    this.socket = socket;
    socket.onopen = () => this.onStatus("open");
    socket.onclose = () => {
      this.socket = null;
      this.onStatus("closed");
      if (!this.closedByUs) setTimeout(() => this.connect(), 2000);
    };
    socket.onmessage = (event: MessageEvent<unknown>) => {
      if (typeof event.data !== "string") return;
      try {
        this.onMessage(JSON.parse(event.data) as ServerMessage);
      } catch {
        // Ignore malformed frames; the gateway only sends generated ServerMessage JSON.
      }
    };
  }

  /** Sends a ping; the pong echoes `client_time_ms`, so RTT needs no bookkeeping here. */
  ping(): boolean {
    if (this.socket?.readyState !== WebSocket.OPEN) return false;
    const message: ClientMessage = {
      type: "ping",
      id: this.nextPingId++,
      client_time_ms: performance.now(),
    };
    this.socket.send(JSON.stringify(message));
    return true;
  }

  close(): void {
    this.closedByUs = true;
    this.socket?.close();
    this.socket = null;
  }
}
