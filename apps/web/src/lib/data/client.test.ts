import type { ResultHeader } from "@lattice/shared-types";
import { tableFromArrays, tableFromIPC, tableToIPC } from "apache-arrow";
import { describe, expect, it } from "vitest";

import { parseFrame } from "./client";

/** Builds a gateway frame: `[u32 LE header length][header JSON][Arrow IPC]`. */
function frame(header: ResultHeader, ipc: Uint8Array): ArrayBuffer {
  const json = new TextEncoder().encode(JSON.stringify(header));
  const out = new Uint8Array(4 + json.length + ipc.length);
  new DataView(out.buffer).setUint32(0, json.length, true);
  out.set(json, 4);
  out.set(ipc, 4 + json.length);
  return out.buffer;
}

const header: ResultHeader = {
  batch_id: 7,
  query_id: "by_region",
  rows: 3,
  truncated: false,
  cached: true,
  columns: [
    { name: "region", role: "dimension" },
    { name: "sum_amount", role: "measure" },
  ],
  fingerprint: "00ff11ee22dd33cc",
  timings: {
    compile_ms: 0.04,
    cache_ms: 0.3,
    queue_ms: 0,
    execute_ms: 0,
    serialize_ms: 0,
    total_ms: 0.4,
  },
};

describe("parseFrame", () => {
  it("splits the header from the Arrow payload", () => {
    const table = tableFromArrays({
      region: ["AMER", "EMEA", "APAC"],
      sum_amount: Float64Array.from([6.1, 3.4, 2.2]),
    });
    const parsed = parseFrame(frame(header, tableToIPC(table, "stream")));
    expect(parsed.header).toEqual(header);
    const decoded = tableFromIPC(parsed.ipc);
    expect(decoded.numRows).toBe(3);
    expect(decoded.getChild("region")?.get(1)).toBe("EMEA");
    expect(decoded.getChild("sum_amount")?.get(2)).toBeCloseTo(2.2);
  });

  it("handles multi-byte characters in the header", () => {
    const parsed = parseFrame(
      frame({ ...header, query_id: "São Paulo · Ümlaut" }, new Uint8Array()),
    );
    expect(parsed.header.query_id).toBe("São Paulo · Ümlaut");
    expect(parsed.ipc.length).toBe(0);
  });
});
