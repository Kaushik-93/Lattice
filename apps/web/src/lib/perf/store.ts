/**
 * Performance samples for the dev overlay (Ctrl+Shift+P).
 *
 * Deliberately NOT React state: recording a sample must never re-render the app it is
 * measuring. Producers push into fixed-size ring buffers; the overlay polls a snapshot a
 * few times per second, and only while it is open.
 */

export class Ring<T> {
  private readonly items: T[] = [];
  private next = 0;

  constructor(readonly capacity: number) {}

  push(item: T): void {
    if (this.items.length < this.capacity) {
      this.items.push(item);
    } else {
      this.items[this.next] = item;
    }
    this.next = (this.next + 1) % this.capacity;
  }

  /** Oldest first. */
  toArray(): T[] {
    if (this.items.length < this.capacity) return [...this.items];
    return [...this.items.slice(this.next), ...this.items.slice(0, this.next)];
  }

  get size(): number {
    return this.items.length;
  }

  clear(): void {
    this.items.length = 0;
    this.next = 0;
  }
}

export interface ServerPhase {
  name: string;
  ms: number;
}

export interface RequestSample {
  path: string;
  /** Wall-clock duration seen by the browser, including network. */
  clientMs: number;
  /** Breakdown from the gateway's Server-Timing header. */
  server: ServerPhase[];
  at: number;
}

export interface QuerySample {
  id: string;
  rows: number;
  cached: boolean;
  /** Gateway total (compile → serialize). */
  serverMs: number;
  /** Browser: batch sent → frame received. */
  clientMs: number;
  decodeMs: number;
  at: number;
}

/** A sheet interaction (filter, cross-filter, drill, sheet switch) until every visual showed
 * its new data. */
export interface SheetSample {
  visuals: number;
  /** Visuals whose data actually changed (the rest were unaffected cache hits). */
  changed: number;
  /** Interaction → last result rendered (the cross-filter budget, spec §3). */
  ms: number;
  at: number;
}

export interface CommitSample {
  id: string;
  phase: string;
  ms: number;
  at: number;
}

export const perf = {
  requests: new Ring<RequestSample>(20),
  rtt: new Ring<number>(50),
  longTasks: new Ring<number>(100),
  commits: new Ring<CommitSample>(50),
  queries: new Ring<QuerySample>(20),
  sheets: new Ring<SheetSample>(20),
  vitals: {} as Partial<Record<"ttfb" | "fcp" | "lcp" | "dcl" | "load", number>>,
};

export function percentile(values: readonly number[], p: number): number | undefined {
  if (values.length === 0) return undefined;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, index)];
}
