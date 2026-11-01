import { tableFromArrays } from "apache-arrow";
import { describe, expect, it } from "vitest";

import type { QueryResult } from "@/lib/data/client";

import { VisualResults, splitQueryId } from "./results";

function result(fingerprint: string): QueryResult {
  return {
    header: {
      batch_id: 1,
      query_id: "v1",
      rows: 1,
      truncated: false,
      cached: false,
      columns: [],
      fingerprint,
      timings: {
        compile_ms: 0,
        cache_ms: 0,
        queue_ms: 0,
        execute_ms: 0,
        serialize_ms: 0,
        total_ms: 0,
      },
    },
    table: tableFromArrays({ a: Float64Array.from([1]) }),
    clientMs: 1,
    decodeMs: 0,
    bytes: 10,
  };
}

describe("visual results", () => {
  it("only notifies a visual when its data changed", () => {
    const results = new VisualResults();
    let renders = 0;
    results.subscribe("v1", () => renders++);
    expect(results.put("v1", "main", result("aa"))).toBe(true);
    // The same data again (an unaffected visual during a cross-filter): no re-render.
    expect(results.put("v1", "main", result("aa"))).toBe(false);
    expect(results.put("v1", "main", result("bb"))).toBe(true);
    results.fail("v1", "boom");
    expect(results.get("v1").error).toBe("boom");
    // A successful result clears the error even with an unchanged fingerprint.
    expect(results.put("v1", "main", result("bb"))).toBe(true);
    expect(renders).toBe(4);
  });

  it("splits compare parts off query ids", () => {
    expect(splitQueryId("abc")).toEqual({ id: "abc", part: "main" });
    expect(splitQueryId("abc:compare")).toEqual({ id: "abc", part: "compare" });
  });
});
