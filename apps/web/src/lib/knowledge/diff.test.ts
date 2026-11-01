import { describe, expect, it } from "vitest";

import { diffLines, sentences } from "./diff";

describe("diffs", () => {
  it("marks removed and added lines around what's kept", () => {
    expect(diffLines("a\nb\nc", "a\nB\nc\nd")).toEqual([
      { kind: "same", text: "a" },
      { kind: "removed", text: "b" },
      { kind: "added", text: "B" },
      { kind: "same", text: "c" },
      { kind: "added", text: "d" },
    ]);
    expect(diffLines("", "x")).toEqual([{ kind: "added", text: "x" }]);
    expect(diffLines("same", "same")).toEqual([{ kind: "same", text: "same" }]);
  });

  it("splits prose into sentences so changes are readable", () => {
    expect(sentences("One. Two!\n\nThree?")).toBe("One.\nTwo!\nThree?");
  });
});
