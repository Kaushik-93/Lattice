import { describe, expect, it } from "vitest";

import { describeFilter, filterFrom, filterText } from "./filters";

describe("chart filters typed into the editor", () => {
  it("parses values by the column's type", () => {
    expect(filterFrom("region", "eq", "VARCHAR", " EMEA ")).toEqual({
      column: "region",
      op: "eq",
      value: "EMEA",
    });
    expect(filterFrom("amount", "gt", "DECIMAL(12,2)", "100")).toEqual({
      column: "amount",
      op: "gt",
      value: 100,
    });
    expect(filterFrom("amount", "gt", "DOUBLE", "lots")).toBeNull();
    expect(filterFrom("region", "in", "VARCHAR", "EMEA, APAC,")).toEqual({
      column: "region",
      op: "in",
      value: ["EMEA", "APAC"],
    });
    expect(filterFrom("order_date", "between", "DATE", "2026-08-01", "2026-10-31")).toEqual({
      column: "order_date",
      op: "between",
      value: ["2026-08-01", "2026-10-31"],
    });
    expect(filterFrom("order_date", "between", "DATE", "2026-08-01")).toBeNull();
    expect(filterFrom("status", "is_null", "VARCHAR", "")).toEqual({
      column: "status",
      op: "is_null",
      value: null,
    });
  });

  it("shows a filter back as text", () => {
    expect(filterText(["EMEA", "APAC"], "in")).toEqual(["EMEA, APAC", ""]);
    expect(filterText([1, 5], "between")).toEqual(["1", "5"]);
    expect(describeFilter({ column: "region", op: "neq", value: "EMEA" })).toBe(
      "region is not EMEA",
    );
    expect(describeFilter({ column: "amount", op: "between", value: [1, 5] })).toBe(
      "amount between 1 and 5",
    );
  });
});
