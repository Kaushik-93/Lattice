import { Int32, Int64, tableFromArrays, vectorFromArray } from "apache-arrow";
import { describe, expect, it } from "vitest";

import { niceTicks, numericValues, preparePoints, timeTicks } from "./points";

describe("scatter points", () => {
  it("normalises coordinates and assigns colour slots by sorted value", () => {
    const table = tableFromArrays({
      x: Float64Array.from([0, 5, 10]),
      y: Float64Array.from([100, 150, 200]),
      region: ["EMEA", "AMER", "APAC"],
    });
    const p = preparePoints(table);
    expect(p.count).toBe(3);
    expect(Array.from(p.positions)).toEqual([0, 0, 0.5, 0.5, 1, 1]);
    expect(p.categories).toEqual(["AMER", "APAC", "EMEA"]);
    expect(Array.from(p.colors)).toEqual([2, 0, 1]);
    expect(p.xTime).toBe(false);
  });

  it("folds categories past three into Other", () => {
    const table = tableFromArrays({
      x: Float64Array.from([1, 2, 3, 4]),
      y: Float64Array.from([1, 2, 3, 4]),
      c: ["a", "b", "c", "d"],
    });
    const p = preparePoints(table);
    expect(p.categories).toEqual(["a", "b", "c", "Other"]);
    expect(p.colors[3]).toBe(3);
  });

  it("reads numbers straight from Arrow's buffers, nulls as NaN", () => {
    const ints = vectorFromArray([1, null, 3], new Int32());
    expect(Array.from(numericValues(ints))).toEqual([1, Number.NaN, 3]);
    const big = vectorFromArray([1n, 2n], new Int64());
    expect(Array.from(numericValues(big))).toEqual([1, 2]);
  });
});

describe("axis ticks", () => {
  it("picks round steps without float noise or -0", () => {
    expect(niceTicks(0, 1000, 4)).toEqual([0, 250, 500, 750, 1000]);
    expect(niceTicks(-0.3, 0.3, 3)).toEqual([-0.2, 0, 0.2]);
    expect(niceTicks(-0.3, 0.3, 3).some((t) => Object.is(t, -0))).toBe(false);
  });

  it("puts time ticks on month boundaries", () => {
    const ticks = timeTicks(Date.UTC(2026, 0, 15), Date.UTC(2026, 6, 1), 6);
    expect(ticks.map((t) => t.label)).toEqual([
      "Feb 26",
      "Mar 26",
      "Apr 26",
      "May 26",
      "Jun 26",
      "Jul 26",
    ]);
  });
});
