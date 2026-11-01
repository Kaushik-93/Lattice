import type { GraphNode, KnowledgeGraph } from "@lattice/shared-types";

/** The gateway's `title_key`: lowercased, whitespace collapsed (crates/knowledge/src/links.rs). */
export function titleKey(title: string): string {
  return title.trim().split(/\s+/).join(" ").toLowerCase();
}

/** Resolves a link title the way the gateway does: a project's own note first, then shared. */
export function resolver(graph: KnowledgeGraph | undefined) {
  const byKey = new Map<string, GraphNode[]>();
  for (const n of graph?.nodes ?? []) {
    const key = titleKey(n.title);
    byKey.set(key, [...(byKey.get(key) ?? []), n]);
  }
  return (title: string, fromProject: string | null = null): GraphNode | undefined => {
    const candidates = byKey.get(titleKey(title)) ?? [];
    return (
      candidates.find((n) => n.project_id !== null && n.project_id === fromProject) ??
      candidates.find((n) => n.project_id === null)
    );
  };
}

/** The `[[partial` being typed just before the caret, if any (for link autocomplete). */
export function linkQueryAt(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const open = before.lastIndexOf("[[");
  if (open === -1) return null;
  const query = before.slice(open + 2);
  if (/[\]\n|]/.test(query) || query.length > 60) return null;
  return { start: open, query };
}
