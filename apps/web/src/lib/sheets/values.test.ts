import { describe, expect, it } from "vitest";

import { formatDate, formatDelta, formatValue, toNumber } from "./values";

describe("values", () => {
  it("formats KPI values like the design", () => {
    expect(formatValue(11_700_000, "currency")).toBe("$11.7M");
    expect(formatValue(86.4, "currency")).toBe("$86.40");
    expect(formatValue(48_210, "number")).toBe("48.2K");
    expect(formatValue(48_210, "number", { compact: false })).toBe("48,210");
    expect(formatValue(0.021, "percent")).toBe("2.1%");
    expect(formatValue(null, "currency")).toBe("—");
  });

  it("applies a visual's decimals, prefix and suffix", () => {
    expect(formatValue(1234.5, "number", { compact: false, decimals: 0 })).toBe("1,235");
    expect(formatValue(0.4567, "percent", { decimals: 2 })).toBe("45.67%");
    expect(formatValue(12, "number", { prefix: "~", suffix: " units" })).toBe("~12 units");
    expect(formatValue(26_600_000, "currency", { decimals: 2 })).toBe("$26.6M");
    expect(formatValue(null, "number", { prefix: "~" })).toBe("—");
  });

  it("describes change between periods", () => {
    expect(formatDelta(105.4, 100)).toEqual({ text: "+5.4%", tone: "up" });
    expect(formatDelta(98.8, 100)).toEqual({ text: "−1.2%", tone: "down" });
    expect(formatDelta(100, 100)).toEqual({ text: "Flat", tone: "flat" });
    expect(formatDelta(1, 0).tone).toBe("flat");
  });

  it("labels dates by their bucket", () => {
    const ms = Date.UTC(2026, 7, 1);
    expect(formatDate(ms, "month")).toBe("Aug 2026");
    expect(formatDate(ms, "quarter")).toBe("Q3 2026");
    expect(formatDate(ms, "year")).toBe("2026");
    expect(formatDate(ms)).toBe("2026-08-01");
  });

  it("reads int64 cells", () => {
    expect(toNumber(3n)).toBe(3);
    expect(toNumber("3")).toBeNull();
  });
});
