/// <reference lib="webworker" />
/**
 * The compute worker: runs packages/wasm off the main thread (spec §3 rule 1). Point indexes
 * stay here between calls, keyed by the visual that owns them, so a hover sends two numbers,
 * not a million points.
 */
import init, { PointIndex, lttb_range } from "@lattice/wasm";

import type { ComputeEnvelope, ComputeReply, ComputeResults } from "./protocol";

const ready = init();
const indexes = new Map<string, PointIndex>();

function handle(req: ComputeEnvelope): ComputeResults[keyof ComputeResults] {
  switch (req.op) {
    case "lttb":
      return lttb_range(req.xs, req.ys, req.start, req.end, req.threshold);
    case "index": {
      indexes.get(req.key)?.free();
      const index = new PointIndex(req.xs, req.ys);
      indexes.set(req.key, index);
      return index.len();
    }
    case "nearest":
      return indexes.get(req.key)?.nearest(req.x, req.y, req.rx, req.ry) ?? -1;
    case "count":
      return indexes.get(req.key)?.count_in(req.x0, req.y0, req.x1, req.y1) ?? 0;
    case "drop":
      indexes.get(req.key)?.free();
      indexes.delete(req.key);
      return null;
  }
}

self.onmessage = async (event: MessageEvent<ComputeEnvelope>) => {
  const req = event.data;
  let reply: ComputeReply;
  try {
    await ready;
    reply = { id: req.id, ok: true, value: handle(req) };
  } catch (error) {
    reply = { id: req.id, ok: false, error: String(error) };
  }
  const transfer = reply.ok && reply.value instanceof Uint32Array ? [reply.value.buffer] : [];
  self.postMessage(reply, transfer);
};
