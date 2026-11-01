import type { ModelEntity, ModelMetric, ProjectModelResponse } from "@lattice/shared-types";
import { describe, expect, it } from "vitest";

import { metricsFor } from "./api";

const definition = (title: string) => ({
  note_id: title,
  title,
  version: 1,
  standing: "approved" as const,
  problem: null,
});

const entity = (title: string, ident: string, tableId: string | null): ModelEntity => ({
  ...definition(title),
  ident,
  table: tableId ? { table_id: tableId, name: ident, columns: [] } : null,
});

const metric = (title: string, grain: string | null): ModelMetric => ({
  ...definition(title),
  formula: null,
  grain,
  uses: [],
});

describe("metrics a visual can chart", () => {
  const model: ProjectModelResponse = {
    fiscal_year_start_month: 2,
    entities: [entity("Order", "order", "orders"), entity("Customer", "customer", null)],
    metrics: [
      metric("Net revenue", "order"),
      metric("Customers", "customer"),
      metric("Broken", null),
    ],
    dimensions: [],
    rules: [],
    relationships: [],
  };

  it("offers the metrics that count the entity bound to the visual's table", () => {
    expect(metricsFor(model, "orders").map((m) => m.title)).toEqual(["Net revenue"]);
    expect(metricsFor(model, "stores")).toEqual([]);
    expect(metricsFor(undefined, "orders")).toEqual([]);
  });
});
