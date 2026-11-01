import { describe, expect, it } from "vitest";

import { casesOf, definitionFrom, definitionLines, fieldsOf } from "./definition";

describe("editing definitions by kind", () => {
  it("round-trips a rule, keeping keys the editor doesn't show", () => {
    const rule = {
      condition: "customer.is_internal = FALSE",
      always: true,
      cases: [{ name: "Staff", rows: {}, count: "customer", expect: 1 }],
      note: "kept",
    };
    const fields = fieldsOf(rule);
    expect(fields.condition).toBe(rule.condition);
    expect(fields.always).toBe(true);
    expect(definitionFrom("rule", fields, rule).definition).toEqual(rule);

    const off = definitionFrom("rule", { ...fields, always: false, cases: "" }, rule);
    expect(off.definition).toEqual({ condition: rule.condition, note: "kept" });
  });

  it("only touches the fields of the note's kind", () => {
    const fields = { ...fieldsOf(null), formula: " SUM(order.amount) ", path: "order.region" };
    expect(definitionFrom("metric", fields, null).definition).toEqual({
      formula: "SUM(order.amount)",
    });
    expect(definitionFrom("dimension", fields, null).definition).toEqual({ path: "order.region" });
    expect(definitionFrom("entity", fields, null).definition).toBeUndefined();
  });

  it("refuses example cases that aren't a JSON list", () => {
    const fields = { ...fieldsOf(null), cases: "{ nope" };
    expect(definitionFrom("metric", fields, null).error).toMatch(/valid JSON/);
    expect(definitionFrom("metric", { ...fields, cases: "{}" }, null).error).toMatch(/list/);
  });

  it("lists a definition line by line for review diffs", () => {
    const d = {
      formula: "SUM(order.amount)",
      cases: [
        { name: "Refunds don't count", expect: 100 },
        { name: "Empty", expect: null },
      ],
    };
    expect(definitionLines(d)).toEqual([
      "SUM(order.amount)",
      'Example "Refunds don\'t count": expect 100',
      'Example "Empty": expect null',
    ]);
    expect(definitionLines(d, { cases: false })).toEqual(["SUM(order.amount)"]);
    expect(definitionLines({ from: "order.customer_id", to: "customer.customer_id" })).toEqual([
      "order.customer_id → customer.customer_id (many to one)",
    ]);
    expect(casesOf({ cases: [{ expect: 1 }] })).toEqual([]);
  });
});
