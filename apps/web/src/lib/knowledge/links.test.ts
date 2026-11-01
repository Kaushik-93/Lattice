import type { KnowledgeGraph } from "@lattice/shared-types";
import { describe, expect, it } from "vitest";

import { linkQueryAt, resolver, titleKey } from "./links";
import { nodeLook } from "./style";

const node = (id: string, title: string, project: string | null = null) => ({
  id,
  kind: "entity" as const,
  title,
  state: "approved" as const,
  project_id: project,
  owner_team: null,
});

describe("links", () => {
  it("normalise titles like the gateway", () => {
    expect(titleKey("  Net   Revenue ")).toBe("net revenue");
  });

  it("resolve a project's own note before a shared one", () => {
    const graph: KnowledgeGraph = {
      nodes: [node("shared", "Order"), node("local", "order", "p1")],
      edges: [],
      unresolved: [],
    };
    const resolve = resolver(graph);
    expect(resolve("ORDER")?.id).toBe("shared");
    expect(resolve("Order", "p1")?.id).toBe("local");
    expect(resolve("Order", "p2")?.id).toBe("shared");
    expect(resolve("Nothing")).toBeUndefined();
  });

  it("find the [[partial link before the caret", () => {
    const text = "A [[Cust";
    expect(linkQueryAt(text, text.length)).toEqual({ start: 2, query: "Cust" });
    expect(linkQueryAt("A [[Customer]] b", 16)).toBeNull();
    expect(linkQueryAt("no link", 7)).toBeNull();
  });

  it("draw proposals in the suggested style whatever their kind", () => {
    expect(nodeLook("metric", "approved").pill).toBe(true);
    expect(nodeLook("metric", "pending")).toMatchObject({
      dashed: true,
      pill: true,
      stroke: "#b45309",
    });
    expect(nodeLook("entity", "deprecated").dashed).toBe(true);
  });
});
