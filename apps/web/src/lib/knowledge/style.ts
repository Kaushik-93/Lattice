import type { NoteKind, NoteState } from "@lattice/shared-types";

/** How a node looks in the graph (design screen 05's legend). */
export interface NodeLook {
  fill: string;
  stroke: string;
  text: string;
  dashed: boolean;
  pill: boolean;
}

const ENTITY: NodeLook = {
  fill: "#ffffff",
  stroke: "#8e97ad",
  text: "#101828",
  dashed: false,
  pill: false,
};
const METRIC: NodeLook = {
  fill: "#eef0ff",
  stroke: "#4338ca",
  text: "#272a6b",
  dashed: false,
  pill: true,
};
const RULE: NodeLook = {
  fill: "#e6f4f1",
  stroke: "#0f766e",
  text: "#0f5f58",
  dashed: false,
  pill: false,
};
const TERM: NodeLook = {
  fill: "#f2f4f7",
  stroke: "#98a2b3",
  text: "#344054",
  dashed: false,
  pill: true,
};
/** Not approved yet: the design's "Suggested" style. */
const PROPOSED: NodeLook = {
  fill: "#fef3e2",
  stroke: "#b45309",
  text: "#8a3b0b",
  dashed: true,
  pill: false,
};
const RETIRED: NodeLook = {
  fill: "#f9fafb",
  stroke: "#d0d5dd",
  text: "#98a2b3",
  dashed: true,
  pill: false,
};
/** A link target no note has yet. */
export const GHOST: NodeLook = {
  fill: "transparent",
  stroke: "#c3c8d4",
  text: "#7a8494",
  dashed: true,
  pill: false,
};

export function nodeLook(kind: NoteKind, state: NoteState): NodeLook {
  const base =
    kind === "metric"
      ? METRIC
      : kind === "entity"
        ? ENTITY
        : kind === "term" || kind === "fact"
          ? TERM
          : RULE;
  switch (state) {
    case "draft":
    case "pending":
      return { ...PROPOSED, pill: base.pill };
    case "rejected":
    case "deprecated":
      return { ...RETIRED, pill: base.pill };
    default:
      return base;
  }
}

/** The small suffix after a node's title in the graph. */
export function stateSuffix(state: NoteState): string | null {
  switch (state) {
    case "draft":
      return "draft";
    case "pending":
      return "pending";
    case "change_draft":
      return "edit";
    case "change_pending":
      return "change pending";
    case "rejected":
      return "rejected";
    case "deprecated":
      return "retired";
    default:
      return null;
  }
}

export const KIND_LABEL: Record<NoteKind, string> = {
  entity: "Entity",
  metric: "Metric",
  dimension: "Dimension",
  rule: "Rule",
  relationship: "Relationship",
  term: "Term",
  fact: "Fact",
};

export const STATE_PILL: Record<NoteState, { label: string; className: string }> = {
  draft: { label: "Draft", className: "bg-secondary text-secondary-foreground" },
  pending: { label: "Needs review", className: "bg-warning-surface text-warning" },
  approved: { label: "Approved", className: "bg-success-surface text-success" },
  change_draft: { label: "Approved · editing", className: "bg-success-surface text-success" },
  change_pending: {
    label: "Approved · change in review",
    className: "bg-warning-surface text-warning",
  },
  rejected: { label: "Rejected", className: "bg-destructive-surface text-destructive" },
  deprecated: { label: "Retired", className: "bg-secondary text-subtle-foreground" },
};

/** Kinds whose definition has a formula (compiled from phase 3.2 on). */
export const COMPUTABLE: NoteKind[] = ["metric", "dimension", "rule", "relationship"];
