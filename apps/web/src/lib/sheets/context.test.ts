import type { SheetFilterDef } from "@lattice/shared-types";
import { describe, expect, it } from "vitest";

import { buildContext, currentValue, filterFor, selectedValues, toggleSelection } from "./context";

const quarter: SheetFilterDef = {
  id: "f1",
  column: "order_date",
  label: "Fiscal Q3",
  kind: "date_range",
  value: ["2026-08-01", "2026-10-31"],
};
const regions: SheetFilterDef = {
  id: "f2",
  column: "region",
  label: "Region",
  kind: "values",
  options: ["AMER", "EMEA"],
};

describe("sheet filters", () => {
  it("uses the saved default until the viewer changes or clears it", () => {
    expect(currentValue(quarter, {})).toEqual(["2026-08-01", "2026-10-31"]);
    expect(currentValue(quarter, { f1: null })).toBeNull();
    expect(currentValue(regions, {})).toBeNull();
  });

  it("turns values into the filters the gateway applies", () => {
    expect(filterFor(quarter, ["2026-01-01", "2026-01-31"])).toEqual({
      column: "order_date",
      op: "between",
      value: ["2026-01-01", "2026-01-31"],
    });
    expect(filterFor(regions, ["EMEA"])).toEqual({ column: "region", op: "eq", value: "EMEA" });
    expect(filterFor(regions, ["EMEA", "AMER"])).toEqual({
      column: "region",
      op: "in",
      value: ["EMEA", "AMER"],
    });
    expect(filterFor(regions, null)).toBeNull();
    expect(filterFor(quarter, ["2026-01-01"])).toBeNull();
  });

  it("builds one context for every visual", () => {
    const context = buildContext([quarter, regions], { f2: ["APAC"] }, []);
    expect(context.filters.map((f) => f.column)).toEqual(["order_date", "region"]);
  });
});

describe("cross-filter selections", () => {
  const pick = (visual: string, value: string) => ({
    visual_id: visual,
    table_id: "t",
    scope: "table" as const,
    filter: { column: "region", op: "eq" as const, value },
  });

  it("replaces a visual's selection, and clicking it again clears it", () => {
    let s = toggleSelection([], pick("a", "EMEA"));
    s = toggleSelection(s, pick("b", "APAC"));
    s = toggleSelection(s, pick("a", "AMER"));
    expect(s.map((x) => x.filter.value)).toEqual(["APAC", "AMER"]);
    s = toggleSelection(s, pick("a", "AMER"));
    expect(s.map((x) => x.visual_id)).toEqual(["b"]);
    expect(selectedValues(s, "b")).toEqual(["APAC"]);
    expect(selectedValues(s, "a")).toEqual([]);
  });
});
