import { describe, expect, it } from "vitest";

import { capitalize, pluralize, updatedLabel } from "./format";

const NOW = new Date("2026-10-31T15:00:00");

describe("updatedLabel", () => {
  it.each([
    ["2026-10-31T14:59:40", "Updated just now"],
    ["2026-10-31T14:56:00", "Updated 4 min ago"],
    ["2026-10-31T12:00:00", "Updated 3 h ago"],
    ["2026-10-30T09:00:00", "Updated yesterday"],
    ["2026-10-28T09:00:00", "Updated Oct 28"],
    ["2025-12-02T09:00:00", "Updated Dec 2, 2025"],
  ])("%s → %s", (iso, label) => {
    expect(updatedLabel(iso, NOW)).toBe(label);
  });
});

describe("text helpers", () => {
  it("capitalizes roles", () => expect(capitalize("editor")).toBe("Editor"));
  it("pluralizes counts", () => {
    expect(pluralize(1, "project")).toBe("1 project");
    expect(pluralize(6, "project")).toBe("6 projects");
  });
});
