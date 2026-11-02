import type { ColorRule } from "@lattice/shared-types";
import { describe, expect, it } from "vitest";

import { mix, paint, rangeOf } from "./rules";

describe("conditional formatting", () => {
  const ranges = [rangeOf([0, 50, 100]), rangeOf([1, 2])];

  it("shades a scale from low to high", () => {
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080");
    const scale: ColorRule[] = [{ kind: "scale", measure: 0, low: "#ffffff", high: "#4338ca" }];
    expect(paint(scale, 0, 100, ranges)).toEqual({ backgroundColor: "#4338ca", color: "#ffffff" });
    expect(paint(scale, 0, 0, ranges)).toEqual({ backgroundColor: "#ffffff", color: "#101828" });
    // Another measure is untouched.
    expect(paint(scale, 1, 2, ranges)).toBeUndefined();
  });

  it("draws bars and highlights values that meet a condition", () => {
    const rules: ColorRule[] = [
      { kind: "bars", measure: 0, color: "#4338ca" },
      { kind: "threshold", measure: 0, op: "lt", value: 10, color: "#b42318" },
    ];
    expect(paint(rules, 0, 50, ranges)).toEqual({
      backgroundImage: "linear-gradient(to right, #4338ca40 50%, transparent 50%)",
    });
    expect(paint(rules, 0, 5, ranges)).toMatchObject({ color: "#b42318", fontWeight: 600 });
    expect(paint(rules, 0, null, ranges)).toBeUndefined();
  });
});
