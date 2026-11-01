import type { VisualSpec } from "@lattice/shared-types";
import { describe, expect, it } from "vitest";

import { bucketEnd, canDrill, drillLevels, selectionFilter, toggleSlicerValue } from "./drill";

const spec = (x: VisualSpec["x"], drill: VisualSpec["drill"] = []): VisualSpec => ({
  kind: "column",
  title: "t",
  table_id: "t",
  x,
  measures: [],
  columns: [],
  drill,
  filters: [],
  options: { stacked: false },
});

describe("drilling", () => {
  it("drills a bucketed date into finer grains, or follows an explicit path", () => {
    const years = drillLevels(spec({ column: "d", bucket: "year" }));
    expect(years.map((l) => l.bucket)).toEqual(["year", "quarter", "month", "day"]);
    const regions = spec({ column: "region", bucket: null }, [
      { column: "category", bucket: null },
    ]);
    expect(drillLevels(regions).map((l) => l.column)).toEqual(["region", "category"]);
    expect(canDrill(regions, 0)).toBe(true);
    expect(canDrill(regions, 1)).toBe(false);
    expect(drillLevels(spec(undefined))).toEqual([]);
  });

  it("knows where each bucket ends", () => {
    expect(bucketEnd("2026-08-01", "month")).toBe("2026-08-31");
    expect(bucketEnd("2028-02-01", "month")).toBe("2028-02-29");
    expect(bucketEnd("2026-07-01", "quarter")).toBe("2026-09-30");
    expect(bucketEnd("2026-01-01", "year")).toBe("2026-12-31");
    expect(bucketEnd("2026-08-03", "week")).toBe("2026-08-09");
    expect(bucketEnd("2026-08-03", "day")).toBe("2026-08-03");
  });

  it("selects a bucket or bin as a range, a category by equality", () => {
    expect(selectionFilter({ column: "d", bucket: "month" }, "2026-08-01")).toEqual({
      column: "d",
      op: "between",
      value: ["2026-08-01", "2026-08-31"],
    });
    expect(selectionFilter({ column: "amount", bucket: null, bin: 50 }, 100)).toEqual({
      column: "amount",
      op: "between",
      value: [100, 150],
    });
    expect(selectionFilter({ column: "region", bucket: null }, "EMEA")).toEqual({
      column: "region",
      op: "eq",
      value: "EMEA",
    });
  });

  it("toggles slicer values in an in-list", () => {
    let f = toggleSlicerValue(undefined, "region", "EMEA");
    f = toggleSlicerValue(f ?? undefined, "region", "APAC");
    expect(f).toEqual({ column: "region", op: "in", value: ["EMEA", "APAC"] });
    f = toggleSlicerValue(f ?? undefined, "region", "EMEA");
    f = toggleSlicerValue(f ?? undefined, "region", "APAC");
    expect(f).toBeNull();
  });
});
