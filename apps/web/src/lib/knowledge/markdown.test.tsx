import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NoteBody, parseBlocks, parseInline } from "./markdown";

describe("note bodies", () => {
  it("parses links, aliases, code and emphasis", () => {
    expect(
      parseInline(
        "From [[Order]]s that are [[Settled orders only|settled]], `status`, **bold**, *em*",
      ),
    ).toEqual([
      { kind: "text", text: "From " },
      { kind: "link", target: "Order", label: "Order" },
      { kind: "text", text: "s that are " },
      { kind: "link", target: "Settled orders only", label: "settled" },
      { kind: "text", text: ", " },
      { kind: "code", text: "status" },
      { kind: "text", text: ", " },
      { kind: "strong", text: "bold" },
      { kind: "text", text: ", " },
      { kind: "em", text: "em" },
    ]);
  });

  it("splits paragraphs and bullet lists", () => {
    const blocks = parseBlocks("One\nline two\n\n- a\n- b");
    expect(blocks.map((b) => b.kind)).toEqual(["paragraph", "list"]);
  });

  it("never renders HTML from a note", () => {
    render(
      <NoteBody
        body={'<img src=x onerror="alert(1)"> [[Order]]'}
        target={() => ({ resolved: true })}
        onLink={() => {}}
      />,
    );
    expect(document.querySelector("img")).toBeNull();
    expect(screen.getByText(/<img src=x/)).toBeTruthy();
  });

  it("links open notes, or offer to create missing ones", async () => {
    const onLink = vi.fn();
    render(
      <NoteBody
        body="See [[Order]] and [[Nowhere]]."
        target={(t) => ({ resolved: t === "Order" })}
        onLink={onLink}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Nowhere" }));
    expect(onLink).toHaveBeenCalledWith("Nowhere");
    expect(screen.getByRole("button", { name: "Nowhere" }).getAttribute("title")).toMatch(
      /create it/,
    );
    expect(screen.getByRole("button", { name: "Order" }).getAttribute("title")).toBe("Open Order");
  });
});
