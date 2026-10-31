import { describe, expect, it } from "vitest";

import { Ring, percentile } from "./store";

describe("Ring", () => {
  it("keeps the newest items, oldest first", () => {
    const ring = new Ring<number>(3);
    [1, 2, 3, 4, 5].forEach((n) => ring.push(n));
    expect(ring.toArray()).toEqual([3, 4, 5]);
    expect(ring.size).toBe(3);
  });

  it("works before it fills up", () => {
    const ring = new Ring<number>(3);
    ring.push(1);
    expect(ring.toArray()).toEqual([1]);
  });
});

describe("percentile", () => {
  it("uses nearest-rank", () => {
    const values = Array.from({ length: 100 }, (_, i) => i + 1);
    expect(percentile(values, 50)).toBe(50);
    expect(percentile(values, 95)).toBe(95);
    expect(percentile([7], 95)).toBe(7);
    expect(percentile([], 95)).toBeUndefined();
  });
});
