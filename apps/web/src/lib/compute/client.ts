import type { ComputeEnvelope, ComputeReply, ComputeRequest, ComputeResults } from "./protocol";

type Pending = { resolve: (value: unknown) => void; reject: (error: Error) => void };

/**
 * Typed calls into the compute worker. One worker per page, started on first use; typed arrays
 * passed in are transferred (the caller must not reuse them).
 */
class Compute {
  private worker: Worker | null = null;
  private next = 1;
  private pending = new Map<number, Pending>();

  private start(): Worker {
    if (this.worker) return this.worker;
    const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<ComputeReply>) => {
      const reply = event.data;
      const pending = this.pending.get(reply.id);
      if (!pending) return;
      this.pending.delete(reply.id);
      if (reply.ok) pending.resolve(reply.value);
      else pending.reject(new Error(reply.error));
    };
    worker.onerror = (event) => {
      for (const [, p] of this.pending)
        p.reject(new Error(event.message || "compute worker failed"));
      this.pending.clear();
    };
    this.worker = worker;
    return worker;
  }

  call<R extends ComputeRequest>(request: R): Promise<ComputeResults[R["op"]]> {
    const worker = this.start();
    const id = this.next++;
    const transfer: Transferable[] = [];
    for (const value of Object.values(request)) {
      if (ArrayBuffer.isView(value)) transfer.push(value.buffer as ArrayBuffer);
    }
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      worker.postMessage({ id, ...request } as ComputeEnvelope, transfer);
    });
  }
}

let shared: Compute | null = null;

export function compute(): Compute {
  shared ??= new Compute();
  return shared;
}

/**
 * Latest-wins wrapper for calls fired on every pointer move: at most one in flight; while it
 * runs, only the newest request waits, the rest are dropped.
 */
export function latestOnly<A extends unknown[], T>(
  fn: (...args: A) => Promise<T>,
  onResult: (value: T) => void,
): (...args: A) => void {
  let running = false;
  let queued: A | null = null;
  const go = (args: A) => {
    running = true;
    fn(...args)
      .then(onResult, () => undefined)
      .finally(() => {
        running = false;
        if (queued) {
          const next = queued;
          queued = null;
          go(next);
        }
      });
  };
  return (...args: A) => {
    if (running) queued = args;
    else go(args);
  };
}
