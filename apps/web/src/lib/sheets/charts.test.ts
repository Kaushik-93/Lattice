import type { VisualSpec } from "@lattice/shared-types";
import { tableFromArrays } from "apache-arrow";
import { describe, expect, it } from "vitest";

import { OTHER, PIE_SLICES, foldSlices, measureLabel, pieOption, shape } from "./charts";

const spec = (over: Partial<VisualSpec>): VisualSpec => ({
  kind: "column",
  title: "t",
  table_id: "t",
  x: { column: "region", bucket: null },
  measures: [{ column: "amount", agg: "sum" }],
  columns: [],
  drill: [],
  filters: [],
  options: { stacked: false },
  ...over,
});

describe("shaping results for charts", () => {
  it("makes one series per measure", () => {
    const table = tableFromArrays({
      region: ["AMER", "EMEA"],
      sum_amount: Float64Array.from([10, 20]),
    });
    const shaped = shape(spec({}), table);
    expect(shaped.labels).toEqual(["AMER", "EMEA"]);
    expect(shaped.series).toEqual([{ name: "Sum of amount", values: [10, 20] }]);
  });

  it("names the gateway's default count when a spec has no measures", () => {
    const table = tableFromArrays({
      amount_bin: Float64Array.from([0, 50]),
      count: BigInt64Array.from([3n, 4n]),
    });
    const shaped = shape(
      spec({ kind: "histogram", x: { column: "amount", bucket: null, bin: 50 }, measures: [] }),
      table,
    );
    expect(shaped.series).toEqual([{ name: "Count", values: [3, 4] }]);
    expect(shaped.labels).toEqual(["0–50", "50–100"]);
  });

  it("pivots a split dimension into aligned series, filling gaps", () => {
    const table = tableFromArrays({
      month: ["Jan", "Jan", "Feb"],
      region: ["AMER", "EMEA", "AMER"],
      sum_amount: Float64Array.from([1, 2, 3]),
    });
    const shaped = shape(spec({ series: { column: "region", bucket: null } }), table);
    expect(shaped.labels).toEqual(["Jan", "Feb"]);
    expect(shaped.series).toEqual([
      { name: "AMER", values: [1, 3] },
      { name: "EMEA", values: [2, null] },
    ]);
  });

  it("folds slices past the palette into Other", () => {
    const n = PIE_SLICES + 3;
    const shaped = {
      keys: Array.from({ length: n }, (_, i) => `k${i}`),
      labels: Array.from({ length: n }, (_, i) => `k${i}`),
      series: [{ name: "Sum", values: Array.from({ length: n }, () => 1) }],
    };
    const folded = foldSlices(shaped);
    expect(folded.labels).toHaveLength(PIE_SLICES);
    expect(folded.labels.at(-1)).toBe("Other");
    expect(folded.keys.at(-1)).toBeUndefined();
    expect(folded.series[0]?.values.at(-1)).toBe(4);
    const table = tableFromArrays({
      k: shaped.keys,
      sum: Float64Array.from(shaped.series[0]!.values),
    });
    const option = pieOption(
      spec({ kind: "pie", measures: [{ column: "sum", agg: "sum" }] }),
      table,
      [],
    );
    const data = (option.series as { data: { itemStyle: { color: string } }[] }[])[0]!.data;
    expect(data.at(-1)?.itemStyle.color).toBe(OTHER);
  });

  it("labels measures", () => {
    expect(measureLabel({ column: null, agg: "count" })).toBe("Count");
    expect(measureLabel({ column: "amount", agg: "avg" })).toBe("Average of amount");
  });
});
