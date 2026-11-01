/** Messages between the page and the compute worker (src/lib/compute/worker.ts). */
export type ComputeRequest =
  | {
      op: "lttb";
      xs: Float64Array;
      ys: Float64Array;
      start: number;
      end: number;
      threshold: number;
    }
  | { op: "index"; key: string; xs: Float32Array; ys: Float32Array }
  | { op: "nearest"; key: string; x: number; y: number; rx: number; ry: number }
  | { op: "count"; key: string; x0: number; y0: number; x1: number; y1: number }
  | { op: "drop"; key: string };

export interface ComputeResults {
  lttb: Uint32Array;
  index: number;
  nearest: number;
  count: number;
  drop: null;
}

export type ComputeEnvelope = { id: number } & ComputeRequest;

export type ComputeReply =
  | { id: number; ok: true; value: ComputeResults[keyof ComputeResults] }
  | { id: number; ok: false; error: string };
