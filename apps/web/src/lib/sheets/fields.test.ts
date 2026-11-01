import type { VisualSpec } from "@lattice/shared-types";
import { describe, expect, it } from "vitest";

import { accepts, placeField, slotsOf, type Field } from "./fields";

const spec = (patch: Partial<VisualSpec>): VisualSpec => ({
  kind: "column",
  title: "t",
  table_id: "orders",
  measures: [],
  columns: [],
  drill: [],
  filters: [],
  options: { stacked: false },
  ...patch,
});

const region: Field = { kind: "column", name: "region", dataType: "VARCHAR" };
const date: Field = { kind: "column", name: "order_date", dataType: "DATE" };
const amount: Field = { kind: "column", name: "amount", dataType: "DECIMAL(12,2)" };
const net: Field = { kind: "metric", id: "net", title: "Net revenue" };
const aov: Field = { kind: "metric", id: "aov", title: "Average order value" };

describe("placing fields on a visual", () => {
  it("knows each kind's slots", () => {
    expect(slotsOf(spec({ kind: "column" }))).toEqual(["axis", "legend", "values", "drill"]);
    expect(slotsOf(spec({ kind: "kpi" }))).toEqual(["values"]);
    expect(slotsOf(spec({ kind: "slicer" }))).toEqual(["axis"]);
  });

  it("puts columns on axes, dates by month", () => {
    const s = placeField(spec({ drill: [{ column: "category", bucket: null }] }), "axis", date);
    expect(s.x).toEqual({ column: "order_date", bucket: "month" });
    expect(s.drill).toEqual([]);
    expect(placeField(s, "legend", region).series).toEqual({ column: "region", bucket: null });
    expect(placeField(s, "drill", region).drill).toEqual([{ column: "region", bucket: null }]);
  });

  it("adds metrics as values, and never mixes them with raw columns", () => {
    let s = placeField(spec({}), "values", net);
    s = placeField(s, "values", aov);
    expect(s.measures.map((m) => m.metric)).toEqual(["net", "aov"]);
    expect(placeField(s, "values", net).measures).toHaveLength(2);
    // A raw column replaces the metrics, and a metric replaces raw columns.
    const raw = placeField(s, "values", amount);
    expect(raw.measures).toEqual([{ column: "amount", agg: "sum" }]);
    expect(placeField(raw, "values", net).measures).toEqual([
      { column: null, agg: "sum", metric: "net" },
    ]);
    expect(placeField(spec({}), "values", region).measures).toEqual([
      { column: "region", agg: "count_distinct" },
    ]);
  });

  it("refuses what doesn't fit", () => {
    expect(accepts(spec({}), "axis", net)).toBe(false);
    expect(accepts(spec({ kind: "kpi" }), "axis", region)).toBe(false);
    expect(accepts(spec({ kind: "histogram" }), "axis", region)).toBe(false);
    expect(placeField(spec({ kind: "histogram" }), "axis", amount, () => 50).x).toEqual({
      column: "amount",
      bucket: null,
      bin: 50,
    });
    // A single-value visual swaps its value.
    const kpi = placeField(spec({ kind: "kpi" }), "values", net);
    expect(placeField(kpi, "values", aov).measures.map((m) => m.metric)).toEqual(["aov"]);
  });
});
