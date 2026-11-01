import type {
  BatchQuery,
  ClientMessage,
  ResultHeader,
  ServerMessage,
  SharedVisualRequest,
  SheetContext,
  VisualRequest,
} from "@lattice/shared-types";
import { tableFromIPC, type Table } from "apache-arrow";

import { GATEWAY_URL } from "@/lib/gateway";
import { perf } from "@/lib/perf/store";

const WS_URL = `${GATEWAY_URL.replace(/^http/, "ws")}/ws`;

export interface QueryResult {
  header: ResultHeader;
  /** Never put this in React state: hold it in a ref or a non-reactive store. */
  table: Table;
  /** Browser-side: request sent → frame received. */
  clientMs: number;
  /** Decoding the Arrow IPC bytes. */
  decodeMs: number;
  bytes: number;
}

export interface QueryFailure {
  queryId: string;
  code: string;
  message: string;
}

export interface BatchOutcome {
  batchId: number;
  results: Map<string, QueryResult>;
  errors: QueryFailure[];
  /** A newer batch replaced this one before it finished. */
  superseded: boolean;
}

interface PendingBatch {
  sentAt: number;
  outcome: BatchOutcome;
  onResult?: (result: QueryResult) => void;
  resolve: (outcome: BatchOutcome) => void;
}

/** Splits a binary frame: `[u32 LE header length][header JSON][Arrow IPC]`. */
export function parseFrame(buffer: ArrayBuffer): { header: ResultHeader; ipc: Uint8Array } {
  const view = new DataView(buffer);
  const headerLength = view.getUint32(0, true);
  const headerBytes = new Uint8Array(buffer, 4, headerLength);
  const header = JSON.parse(new TextDecoder().decode(headerBytes)) as ResultHeader;
  return { header, ipc: new Uint8Array(buffer, 4 + headerLength) };
}

/**
 * The browser's data connection to the gateway. One per page: all of a view's queries go out
 * in one batch, results stream back as Arrow as each completes, and sending a new batch
 * cancels the previous one end to end (spec §3 rules 6–7).
 */
export class DataClient {
  private socket: WebSocket | null = null;
  private ready: Promise<WebSocket> | null = null;
  private nextBatchId = 1;
  private pending = new Map<number, PendingBatch>();

  private connect(): Promise<WebSocket> {
    if (this.ready) return this.ready;
    this.ready = new Promise<WebSocket>((resolve, reject) => {
      const socket = new WebSocket(WS_URL);
      socket.binaryType = "arraybuffer";
      socket.onopen = () => {
        this.socket = socket;
        resolve(socket);
      };
      socket.onerror = () => reject(new Error("Can't reach the Lattice gateway."));
      socket.onclose = () => {
        this.socket = null;
        this.ready = null;
        for (const [, batch] of this.pending) {
          batch.outcome.errors.push({
            queryId: "*",
            code: "disconnected",
            message: "Connection to the gateway was lost.",
          });
          batch.resolve(batch.outcome);
        }
        this.pending.clear();
      };
      socket.onmessage = (event: MessageEvent<ArrayBuffer | string>) => this.onMessage(event.data);
    });
    return this.ready;
  }

  /** Sends table queries as one batch. Earlier unfinished batches resolve as `superseded`. */
  run(
    projectId: string,
    queries: BatchQuery[],
    onResult?: (result: QueryResult) => void,
  ): Promise<BatchOutcome> {
    return this.send(
      (batchId) => ({ type: "query_batch", batch_id: batchId, project_id: projectId, queries }),
      onResult,
    );
  }

  /**
   * Sends a sheet's visuals with its interactive state; the gateway compiles them. Results are
   * keyed by visual id, plus `:compare` for a KPI's previous period.
   */
  runSheet(
    projectId: string,
    context: SheetContext,
    visuals: VisualRequest[],
    onResult?: (result: QueryResult) => void,
  ): Promise<BatchOutcome> {
    return this.send(
      (batchId) => ({
        type: "sheet_batch",
        batch_id: batchId,
        project_id: projectId,
        context,
        visuals,
      }),
      onResult,
    );
  }

  /** Runs the saved visuals of a sheet opened through its share link. */
  runShared(
    token: string,
    context: SheetContext,
    visuals: SharedVisualRequest[],
    onResult?: (result: QueryResult) => void,
  ): Promise<BatchOutcome> {
    return this.send(
      (batchId) => ({ type: "shared_batch", batch_id: batchId, token, context, visuals }),
      onResult,
    );
  }

  private async send(
    build: (batchId: number) => ClientMessage,
    onResult?: (result: QueryResult) => void,
  ): Promise<BatchOutcome> {
    const socket = await this.connect();
    for (const [, batch] of this.pending) {
      batch.outcome.superseded = true;
      batch.resolve(batch.outcome);
    }
    this.pending.clear();

    const batchId = this.nextBatchId++;
    const message = build(batchId);
    return new Promise<BatchOutcome>((resolve) => {
      this.pending.set(batchId, {
        sentAt: performance.now(),
        outcome: { batchId, results: new Map(), errors: [], superseded: false },
        onResult,
        resolve,
      });
      socket.send(JSON.stringify(message));
    });
  }

  close(): void {
    this.socket?.close();
  }

  private onMessage(data: ArrayBuffer | string): void {
    if (typeof data !== "string") {
      const receivedAt = performance.now();
      const { header, ipc } = parseFrame(data);
      const batch = this.pending.get(header.batch_id);
      if (!batch) return; // a superseded batch's straggler
      const decodeStart = performance.now();
      const table = tableFromIPC(ipc);
      const result: QueryResult = {
        header,
        table,
        clientMs: receivedAt - batch.sentAt,
        decodeMs: performance.now() - decodeStart,
        bytes: data.byteLength,
      };
      batch.outcome.results.set(header.query_id, result);
      perf.queries.push({
        id: header.query_id,
        rows: header.rows,
        cached: header.cached,
        serverMs: header.timings.total_ms,
        clientMs: result.clientMs,
        decodeMs: result.decodeMs,
        at: receivedAt,
      });
      batch.onResult?.(result);
      return;
    }
    let message: ServerMessage;
    try {
      message = JSON.parse(data) as ServerMessage;
    } catch {
      return;
    }
    if (message.type === "query_error") {
      this.pending.get(message.batch_id)?.outcome.errors.push({
        queryId: message.query_id,
        code: message.code,
        message: message.message,
      });
    } else if (message.type === "error") {
      // The gateway ends the socket after this (e.g. an expired session): fail what's waiting.
      for (const [, batch] of this.pending) {
        batch.outcome.errors.push({ queryId: "*", code: message.code, message: message.message });
        batch.resolve(batch.outcome);
      }
      this.pending.clear();
    } else if (message.type === "batch_done") {
      const batch = this.pending.get(message.batch_id);
      if (batch) {
        batch.outcome.superseded = batch.outcome.superseded || message.cancelled;
        this.pending.delete(message.batch_id);
        batch.resolve(batch.outcome);
      }
    }
  }
}

const clients = new Map<string, DataClient>();

/**
 * A data connection, opened on first use. Each channel is its own socket, so its batches
 * don't cancel another's: the sheet's batch and a table visual's next page run side by side.
 */
export function dataClient(channel = "main"): DataClient {
  let client = clients.get(channel);
  if (!client) {
    client = new DataClient();
    clients.set(channel, client);
  }
  return client;
}
