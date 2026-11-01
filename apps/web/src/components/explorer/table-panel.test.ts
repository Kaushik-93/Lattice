import type { SourceTable } from "@lattice/shared-types";
import { describe, expect, it } from "vitest";

import { isNumeric, isTemporal, toQuery } from "./table-panel";

const table: SourceTable = {
  id: "t1",
  schema_name: "s_1",
  name: "orders",
  mode: "extract",
  row_count: 1000,
  profiled_at: null,
  columns: [
    { name: "order_date", data_type: "DATE", nullable: true, profile: null },
    { name: "region", data_type: "VARCHAR", nullable: true, profile: null },
    { name: "store_id", data_type: "INTEGER", nullable: true, profile: null },
    { name: "amount", data_type: "DECIMAL(12,2)", nullable: true, profile: null },
  ],
};

const base = {
  mode: "summary" as const,
  dimension: "region",
  bucket: "" as const,
  agg: "sum" as const,
  measure: "amount",
  filterColumn: "",
  filterOp: "eq" as const,
  filterValue: "",
  limit: 100,
};

describe("toQuery", () => {
  it("compiles a grouped summary sorted by the measure", () => {
    expect(toQuery(table, base)).toEqual({
      table_id: "t1",
      dimensions: [{ column: "region", bucket: null }],
      measures: [{ column: "amount", agg: "sum" }],
      filters: [],
      sort: [{ by: "sum_amount", dir: "desc" }],
      limit: 100,
      offset: null,
    });
  });

  it("time buckets read chronologically (no measure sort)", () => {
    const q = toQuery(table, { ...base, dimension: "order_date", bucket: "month", agg: "count" });
    expect(q.dimensions).toEqual([{ column: "order_date", bucket: "month" }]);
    expect(q.measures).toEqual([{ column: null, agg: "count" }]);
    expect(q.sort).toEqual([]);
  });

  it("rows mode ignores grouping; numeric filters bind numbers", () => {
    const q = toQuery(table, {
      ...base,
      mode: "rows",
      filterColumn: "store_id",
      filterOp: "gte",
      filterValue: " 42 ",
      limit: 1000,
    });
    expect(q.dimensions).toEqual([]);
    expect(q.measures).toEqual([]);
    expect(q.filters).toEqual([{ column: "store_id", op: "gte", value: 42 }]);
    expect(q.limit).toBe(1000);
  });

  it("text filters stay text, blank filters are dropped", () => {
    expect(
      toQuery(table, { ...base, filterColumn: "region", filterValue: "EMEA" }).filters,
    ).toEqual([{ column: "region", op: "eq", value: "EMEA" }]);
    expect(toQuery(table, { ...base, filterColumn: "region", filterValue: "  " }).filters).toEqual(
      [],
    );
  });
});

describe("type helpers", () => {
  it("classify DuckDB types", () => {
    expect(isNumeric("DECIMAL(12,2)")).toBe(true);
    expect(isNumeric("HUGEINT")).toBe(true);
    expect(isNumeric("VARCHAR")).toBe(false);
    expect(isTemporal("TIMESTAMP WITH TIME ZONE")).toBe(true);
    expect(isTemporal("DATE")).toBe(true);
    expect(isTemporal("INTEGER")).toBe(false);
  });
});
