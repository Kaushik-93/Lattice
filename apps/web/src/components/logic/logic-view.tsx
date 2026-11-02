"use client";

import type { NoteDetail } from "@lattice/shared-types";
import { Search } from "lucide-react";
import { useSearchParams } from "next/navigation";
import { useCallback, useEffect, useId, useState } from "react";

import { isSignedOutError } from "@/components/shell/auth-gate";
import { ErrorState } from "@/components/shell/page-skeleton";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useKnowledgeGraph, useReviewQueue } from "@/lib/knowledge/api";
import { resolver } from "@/lib/knowledge/links";
import { useMe, useProjects } from "@/lib/queries";
import { cn } from "@/lib/utils";

import { KnowledgeGraph } from "./knowledge-graph";
import { NoteEditor, type EditorTarget } from "./note-editor";
import { NotePanel } from "./note-panel";
import { ReviewQueue } from "./review-queue";
import { Suggestions } from "./suggestions";

const LEGEND: { label: string; className: string }[] = [
  { label: "Entity", className: "rounded-[4px] border-[#8e97ad] bg-white" },
  { label: "Metric", className: "rounded-full border-[#4338ca] bg-[#eef0ff]" },
  { label: "Rule / dimension", className: "rounded-[4px] border-[#0f766e] bg-[#e6f4f1]" },
  {
    label: "Not approved yet",
    className: "rounded-[4px] border-dashed border-[#b45309] bg-[#fef3e2]",
  },
  { label: "Not written yet", className: "rounded-[4px] border-dashed border-[#c3c8d4] bg-white" },
];

/** Design screen 05: the workspace's knowledge as a graph you can explore, review and edit. */
export function LogicView() {
  const params = useSearchParams();
  const me = useMe();
  const projects = useProjects();
  const [project, setProject] = useState<string | null>(params.get("project"));
  const graph = useKnowledgeGraph(project);
  const queue = useReviewQueue();
  const [selected, setSelected] = useState<string | null>(params.get("note"));
  const [editor, setEditor] = useState<EditorTarget | null>(null);
  const [search, setSearch] = useState("");
  const [local, setLocal] = useState(false);
  const searchId = useId();
  const scopeId = useId();

  // The open note is in the URL, so a note can be linked to.
  useEffect(() => {
    const url = new URL(window.location.href);
    if (selected) url.searchParams.set("note", selected);
    else url.searchParams.delete("note");
    if (project) url.searchParams.set("project", project);
    else url.searchParams.delete("project");
    window.history.replaceState(null, "", url);
  }, [selected, project]);

  const resolve = resolver(graph.data);
  const openLink = useCallback(
    (title: string, from?: NoteDetail) => {
      const target = resolve(title, from?.project_id ?? null);
      if (target) setSelected(target.id);
      else setEditor({ mode: "new", title, projectId: from?.project_id ?? null });
    },
    [resolve],
  );
  const onGhost = useCallback((title: string) => setEditor({ mode: "new", title }), []);
  const onSelect = useCallback((id: string | null) => setSelected(id), []);

  if (graph.isError && !isSignedOutError(graph.error)) {
    return <ErrorState title="Couldn't load business logic" message={graph.error.message} />;
  }
  const nodes = graph.data?.nodes ?? [];
  const waiting = (queue.data ?? []).filter((i) => i.revision.status === "pending").length;
  const matches = search.trim()
    ? nodes.filter((n) => n.title.toLowerCase().includes(search.trim().toLowerCase()))
    : [];

  return (
    <>
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="text-[13px] text-subtle-foreground">
            {me.data?.workspace.name ?? "Workspace"} · organization-wide
          </span>
          <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">
            Business logic
          </h1>
          <p className="max-w-[720px] text-[15px] text-muted-foreground">
            Your organization&apos;s shared business knowledge — entities, metrics, rules and how
            they connect — that every project and every answer is built on. Nothing is used until
            its owner approves it.
          </p>
          <p className="text-[13px] text-subtle-foreground">
            {nodes.length} notes · {nodes.filter((n) => n.kind === "entity").length} entities ·{" "}
            {waiting} waiting for review
          </p>
        </div>
        <Button onClick={() => setEditor({ mode: "new", projectId: project })}>+ New note</Button>
      </header>

      <div className="grid items-stretch gap-4 lg:grid-cols-[minmax(0,1fr)_380px]">
        <section
          aria-labelledby="graph-h"
          className="flex min-h-[560px] flex-col overflow-hidden rounded-xl border border-border bg-card"
        >
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-border-subtle px-5 py-3">
            <h2 id="graph-h" className="text-[15px] font-semibold">
              Knowledge graph
            </h2>
            <ul
              className="flex flex-wrap gap-3 text-xs text-secondary-foreground"
              aria-label="Legend"
            >
              {LEGEND.map((l) => (
                <li key={l.label} className="flex items-center gap-1.5">
                  <span aria-hidden className={cn("h-3 w-4 border", l.className)} />
                  {l.label}
                </li>
              ))}
            </ul>
          </div>
          <div className="flex flex-wrap items-center gap-2 px-5 py-2.5">
            <label htmlFor={searchId} className="relative flex-1">
              <span className="sr-only">Find a note</span>
              <Search
                className="absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-subtle-foreground"
                aria-hidden
              />
              <input
                id={searchId}
                type="search"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && matches[0]) setSelected(matches[0].id);
                }}
                placeholder="Find a note…"
                className="h-9 w-full rounded-md border border-input bg-card pr-3 pl-8 text-sm"
              />
            </label>
            <label htmlFor={scopeId} className="sr-only">
              Show
            </label>
            <select
              id={scopeId}
              value={project ?? ""}
              onChange={(e) => setProject(e.target.value || null)}
              className="h-9 rounded-md border border-input bg-card px-2 text-sm"
            >
              <option value="">Whole workspace</option>
              {(projects.data ?? []).map((p) => (
                <option key={p.id} value={p.id}>
                  Shared + {p.name}
                </option>
              ))}
            </select>
            <button
              type="button"
              aria-pressed={local}
              disabled={!selected}
              onClick={() => setLocal((v) => !v)}
              className={cn(
                "h-9 rounded-md border px-3 text-sm font-medium disabled:opacity-50",
                local
                  ? "border-primary bg-accent text-accent-foreground"
                  : "border-input hover:bg-secondary",
              )}
              title="Show only the selected note's neighbourhood"
            >
              Local graph
            </button>
          </div>
          <div className="relative min-h-0 flex-1">
            {graph.data ? (
              graph.data.nodes.length === 0 ? (
                <div className="grid size-full place-items-center p-8 text-center text-sm text-muted-foreground">
                  No notes yet. Write the first one — an entity like “Customer”, a metric, or a
                  rule.
                </div>
              ) : (
                <KnowledgeGraph
                  graph={graph.data}
                  selected={selected}
                  onSelect={onSelect}
                  onGhost={onGhost}
                  search={search}
                  local={local}
                />
              )
            ) : (
              <Skeleton className="absolute inset-4" />
            )}
          </div>
        </section>
        <aside aria-label="Selected note" className="rounded-xl border border-border bg-card">
          {selected ? (
            <NotePanel
              id={selected}
              onOpenLink={openLink}
              onEdit={(n) => setEditor({ mode: "edit", noteId: n.id })}
            />
          ) : (
            <div className="flex h-full flex-col justify-center gap-2 p-6 text-sm text-muted-foreground">
              <p className="font-medium text-foreground">Pick a note in the graph</p>
              <p>
                Drag to rearrange, scroll to zoom, hover to see what a note connects to. Dashed
                notes aren&apos;t approved yet; faint italic ones are linked but not written.
              </p>
            </div>
          )}
        </aside>
      </div>

      <div className="grid items-start gap-4 lg:grid-cols-[380px_minmax(0,1fr)]">
        <div className="flex flex-col gap-6">
          <ReviewQueue
            onOpen={setSelected}
            onEdit={(id) => setEditor({ mode: "edit", noteId: id })}
          />
          <Suggestions onOpen={setSelected} />
        </div>
        {editor ? (
          <NoteEditor
            target={editor}
            graph={graph.data}
            projects={projects.data ?? []}
            onClose={() => setEditor(null)}
            onSaved={(id) => {
              setSelected(id);
              setEditor({ mode: "edit", noteId: id });
            }}
          />
        ) : (
          <div className="rounded-xl border border-dashed border-input p-6 text-sm text-muted-foreground">
            Open a note in the editor to change it — the approved version keeps serving until the
            change is approved — or start a new one.
          </div>
        )}
      </div>
    </>
  );
}
