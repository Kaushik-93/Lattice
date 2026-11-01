"use client";

import type { KnowledgeGraph, NoteDetail, NoteKind, ProjectSummary } from "@lattice/shared-types";
import { X } from "lucide-react";
import { useId, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { useCreateNote, useNote, useNoteAction } from "@/lib/knowledge/api";
import { diffLines, sentences, type DiffLine } from "@/lib/knowledge/diff";
import { linkQueryAt } from "@/lib/knowledge/links";
import { COMPUTABLE, KIND_LABEL, STATE_PILL } from "@/lib/knowledge/style";
import { cn } from "@/lib/utils";

import { formulaOf } from "./note-panel";

export type EditorTarget =
  { mode: "new"; title?: string; projectId?: string | null } | { mode: "edit"; noteId: string };

const KINDS = Object.keys(KIND_LABEL) as NoteKind[];
const field =
  "w-full rounded-[10px] border border-input bg-card px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

/** Design screen 05's editor card: writing a note or a change to one. */
export function NoteEditor({
  target,
  graph,
  projects,
  onClose,
  onSaved,
}: {
  target: EditorTarget;
  graph: KnowledgeGraph | undefined;
  projects: ProjectSummary[];
  onClose: () => void;
  onSaved: (noteId: string) => void;
}) {
  const existing = useNote(target.mode === "edit" ? target.noteId : null);
  if (target.mode === "edit" && !existing.data) return null;
  return (
    <EditorForm
      key={
        target.mode === "edit"
          ? `${target.noteId}:${existing.data?.open?.id ?? existing.data?.live?.id}`
          : `new:${target.title ?? ""}`
      }
      target={target}
      note={existing.data ?? null}
      graph={graph}
      projects={projects}
      onClose={onClose}
      onSaved={onSaved}
    />
  );
}

function EditorForm({
  target,
  note,
  graph,
  projects,
  onClose,
  onSaved,
}: {
  target: EditorTarget;
  note: NoteDetail | null;
  graph: KnowledgeGraph | undefined;
  projects: ProjectSummary[];
  onClose: () => void;
  onSaved: (noteId: string) => void;
}) {
  const base = note?.open ?? note?.live ?? null;
  const [tab, setTab] = useState<"logic" | "history">("logic");
  const [kind, setKind] = useState<NoteKind>(note?.kind ?? "metric");
  const [scope, setScope] = useState<string>(target.mode === "new" ? (target.projectId ?? "") : "");
  const [team, setTeam] = useState(note?.owner.team ?? "");
  const [title, setTitle] = useState(
    base?.title ?? (target.mode === "new" ? (target.title ?? "") : ""),
  );
  const [body, setBody] = useState(base?.body ?? "");
  const [formula, setFormula] = useState(formulaOf(base));
  const [changeNote, setChangeNote] = useState(note?.open?.change_note ?? "");
  const create = useCreateNote();
  const action = useNoteAction(note?.id ?? "");
  const ids = {
    title: useId(),
    body: useId(),
    formula: useId(),
    note: useId(),
    kind: useId(),
    scope: useId(),
    team: useId(),
  };
  const computable = COMPUTABLE.includes(kind);
  const error = create.error ?? action.error;
  const busy = create.isPending || action.isPending;

  const definition = computable && formula.trim() ? { formula: formula.trim() } : undefined;
  const save = async (submit: boolean) => {
    if (!note) {
      const created = await create.mutateAsync({
        kind,
        title,
        body,
        definition,
        project_id: scope || undefined,
        owner_team: team.trim() || undefined,
        submit,
      });
      onSaved(created.id);
      return;
    }
    await action.mutateAsync({
      kind: "draft",
      edit: { title, body, definition, change_note: changeNote.trim() || undefined },
    });
    if (submit) await action.mutateAsync({ kind: "submit" });
    onSaved(note.id);
  };

  const live = note?.live;
  const bodyDiff = useMemo(
    () => (live ? diffLines(sentences(live.body), sentences(body)) : []),
    [live, body],
  );
  const formulaDiff = useMemo(
    () => (live ? diffLines(formulaOf(live), formula) : []),
    [live, formula],
  );
  const changed = (d: DiffLine[]) => d.some((l) => l.kind !== "same");

  return (
    <section
      aria-label="Note editor"
      className="flex flex-col rounded-xl border border-border bg-card"
      data-testid="note-editor"
    >
      <div className="flex items-start justify-between gap-3 px-6 pt-5">
        <div className="flex flex-col gap-0.5">
          <span className="text-xs text-subtle-foreground">
            {note
              ? `Editing · ${note.open ? `${note.open.status} v${note.open.version}` : `new version of v${note.live?.version ?? 1}`}`
              : "New note"}
          </span>
          <h2 className="text-lg font-semibold">{title || "Untitled"}</h2>
        </div>
        <button
          type="button"
          aria-label="Close the editor"
          onClick={onClose}
          className="grid size-8 place-items-center rounded-md hover:bg-secondary"
        >
          <X className="size-4" />
        </button>
      </div>
      {note && (
        <div
          role="tablist"
          aria-label="Editor"
          className="mx-6 mt-3 flex gap-1 border-b border-border"
        >
          {(["logic", "history"] as const).map((t) => (
            <button
              key={t}
              type="button"
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={cn(
                "-mb-px border-b-2 px-3 py-2 text-sm font-medium",
                tab === t
                  ? "border-primary text-accent-foreground"
                  : "border-transparent text-muted-foreground hover:text-foreground",
              )}
            >
              {t === "logic" ? "Logic" : "Version history"}
            </button>
          ))}
        </div>
      )}
      {tab === "history" && note ? (
        <History note={note} />
      ) : (
        <form
          className="flex flex-col gap-4 px-6 py-5"
          onSubmit={(e) => {
            e.preventDefault();
            void save(false);
          }}
        >
          {!note && (
            <div className="grid grid-cols-[repeat(auto-fit,minmax(150px,1fr))] gap-3">
              <label htmlFor={ids.kind} className="grid gap-1.5 text-[13px] font-semibold">
                Kind
                <select
                  id={ids.kind}
                  className={field}
                  value={kind}
                  onChange={(e) => setKind(e.target.value as NoteKind)}
                >
                  {KINDS.map((k) => (
                    <option key={k} value={k}>
                      {KIND_LABEL[k]}
                    </option>
                  ))}
                </select>
              </label>
              <label htmlFor={ids.scope} className="grid gap-1.5 text-[13px] font-semibold">
                Applies to
                <select
                  id={ids.scope}
                  className={field}
                  value={scope}
                  onChange={(e) => setScope(e.target.value)}
                >
                  <option value="">The whole workspace</option>
                  {projects
                    .filter((p) => p.my_role !== "viewer")
                    .map((p) => (
                      <option key={p.id} value={p.id}>
                        Only {p.name}
                      </option>
                    ))}
                </select>
              </label>
              <label htmlFor={ids.team} className="grid gap-1.5 text-[13px] font-semibold">
                Owning team
                <input
                  id={ids.team}
                  className={field}
                  value={team}
                  maxLength={60}
                  placeholder="e.g. Finance"
                  onChange={(e) => setTeam(e.target.value)}
                />
              </label>
            </div>
          )}
          <label htmlFor={ids.title} className="grid gap-1.5 text-[13px] font-semibold">
            Title
            <input
              id={ids.title}
              className={field}
              value={title}
              maxLength={120}
              required
              onChange={(e) => setTitle(e.target.value)}
            />
          </label>
          <div className="grid gap-1.5">
            <label htmlFor={ids.body} className="text-[13px] font-semibold">
              Definition in plain English
            </label>
            <LinkTextarea id={ids.body} value={body} onChange={setBody} graph={graph} />
            <span className="text-xs text-muted-foreground">
              Type [[ to link another note, e.g. [[Customer]].
            </span>
          </div>
          {computable && (
            <label htmlFor={ids.formula} className="grid gap-1.5 text-[13px] font-semibold">
              Formula
              <textarea
                id={ids.formula}
                rows={3}
                value={formula}
                onChange={(e) => setFormula(e.target.value)}
                placeholder="SUM(order.amount) WHERE order.status = 'settled'"
                className={cn(field, "font-mono text-[13px]")}
              />
              <span className="text-xs font-normal text-muted-foreground">
                Checked and compiled to SQL once the expression language lands (slice 3.2).
              </span>
            </label>
          )}
          {live && (changed(bodyDiff) || changed(formulaDiff)) && (
            <div className="grid gap-1.5" data-testid="note-diff">
              <span className="text-[13px] font-semibold">
                Changes from the published version (v{live.version})
              </span>
              <DiffBlock
                lines={[
                  ...bodyDiff,
                  ...(changed(formulaDiff)
                    ? [{ kind: "same" as const, text: "" }, ...formulaDiff]
                    : []),
                ]}
              />
            </div>
          )}
          {note && (
            <label htmlFor={ids.note} className="grid gap-1.5 text-[13px] font-semibold">
              Why this change?
              <input
                id={ids.note}
                className={field}
                value={changeNote}
                maxLength={500}
                onChange={(e) => setChangeNote(e.target.value)}
                placeholder="Shown to the reviewer"
              />
            </label>
          )}
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error.message}
            </p>
          )}
          <div className="flex flex-wrap justify-end gap-2 border-t border-border-subtle pt-4">
            <Button type="submit" variant="outline" disabled={busy || !title.trim()}>
              Save draft
            </Button>
            <Button type="button" disabled={busy || !title.trim()} onClick={() => void save(true)}>
              Submit for review
            </Button>
          </div>
        </form>
      )}
    </section>
  );
}

function DiffBlock({ lines }: { lines: DiffLine[] }) {
  return (
    <pre className="overflow-x-auto rounded-[10px] bg-brand-ink py-2 font-mono text-[13px] leading-relaxed whitespace-pre-wrap text-white">
      {lines.map((l, i) => (
        <div
          key={i}
          className={cn(
            "px-4",
            l.kind === "removed" && "bg-[#4a2a1a] text-[#ffcfa8]",
            l.kind === "added" && "bg-[#1f2a5c] text-[#d6dbff]",
            l.kind === "same" && "text-white/70",
          )}
        >
          <span aria-hidden className="mr-3 inline-block w-3 select-none">
            {l.kind === "removed" ? "−" : l.kind === "added" ? "+" : ""}
          </span>
          <span className="sr-only">
            {l.kind === "removed" ? "Removed: " : l.kind === "added" ? "Added: " : ""}
          </span>
          {l.text}
        </div>
      ))}
    </pre>
  );
}

/** A textarea that suggests note titles after "[[" (Obsidian's link autocomplete). */
function LinkTextarea({
  id,
  value,
  onChange,
  graph,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
  graph: KnowledgeGraph | undefined;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const [caret, setCaret] = useState(0);
  const [active, setActive] = useState(0);
  const query = linkQueryAt(value, caret);
  const suggestions = useMemo(() => {
    if (!query) return [];
    const q = query.query.trim().toLowerCase();
    const titles = [...new Set((graph?.nodes ?? []).map((n) => n.title))];
    return titles
      .filter((t) => t.toLowerCase().includes(q))
      .sort(
        (a, b) =>
          Number(!a.toLowerCase().startsWith(q)) - Number(!b.toLowerCase().startsWith(q)) ||
          a.localeCompare(b),
      )
      .slice(0, 6);
  }, [query, graph]);

  const pick = (title: string) => {
    if (!query) return;
    const next = `${value.slice(0, query.start)}[[${title}]]${value.slice(caret)}`;
    onChange(next);
    const at = query.start + title.length + 4;
    requestAnimationFrame(() => {
      ref.current?.focus();
      ref.current?.setSelectionRange(at, at);
      setCaret(at);
    });
  };

  return (
    <div className="relative">
      <textarea
        ref={ref}
        id={id}
        rows={4}
        value={value}
        aria-autocomplete="list"
        aria-controls={suggestions.length ? `${id}-links` : undefined}
        onChange={(e) => {
          onChange(e.target.value);
          setCaret(e.target.selectionStart);
          setActive(0);
        }}
        onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
        onKeyDown={(e) => {
          if (!suggestions.length) return;
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            setActive(
              (a) =>
                (a + (e.key === "ArrowDown" ? 1 : suggestions.length - 1)) % suggestions.length,
            );
          } else if (e.key === "Enter" || e.key === "Tab") {
            e.preventDefault();
            pick(suggestions[active] ?? suggestions[0]!);
          } else if (e.key === "Escape") {
            setCaret(-1);
          }
        }}
        className={field}
      />
      {suggestions.length > 0 && (
        <ul
          id={`${id}-links`}
          role="listbox"
          aria-label="Link to a note"
          className="absolute z-10 mt-1 w-64 rounded-[10px] border border-border bg-popover p-1 shadow-md"
        >
          {suggestions.map((s, i) => (
            <li key={s} role="option" aria-selected={i === active}>
              <button
                type="button"
                onMouseDown={(e) => {
                  e.preventDefault();
                  pick(s);
                }}
                className={cn(
                  "w-full rounded-md px-2.5 py-1.5 text-left text-sm",
                  i === active ? "bg-accent text-accent-foreground" : "hover:bg-secondary",
                )}
              >
                {s}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function History({ note }: { note: NoteDetail }) {
  const action = useNoteAction(note.id);
  return (
    <ol className="flex flex-col px-6 py-4" aria-label="Versions">
      {note.history.map((r) => {
        const pill =
          STATE_PILL[
            r.status === "approved"
              ? "approved"
              : r.status === "pending"
                ? "pending"
                : r.status === "rejected"
                  ? "rejected"
                  : r.status === "draft"
                    ? "draft"
                    : "deprecated"
          ];
        const restorable =
          (r.status === "superseded" || r.status === "rejected") && !note.open && note.can.edit;
        return (
          <li
            key={r.id}
            className="flex flex-col gap-1 border-b border-border-subtle py-3 last:border-0"
          >
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <span className="font-semibold">v{r.version}</span>
              <span className={cn("rounded-full px-2 py-0.5 text-xs", pill.className)}>
                {r.status === "superseded" ? "Replaced" : pill.label}
              </span>
              <span className="text-muted-foreground">
                {r.author_name ?? "Unknown"} ·{" "}
                {new Date(r.updated_at).toLocaleDateString("en-US", {
                  month: "short",
                  day: "numeric",
                  year: "numeric",
                })}
              </span>
              {restorable && (
                <Button
                  size="sm"
                  variant="ghost"
                  className="ml-auto"
                  onClick={() => action.mutate({ kind: "revert", version: r.version })}
                >
                  Restore as draft
                </Button>
              )}
            </div>
            {r.change_note && <p className="text-sm text-secondary-foreground">{r.change_note}</p>}
            {r.review_reason && (
              <p className="text-sm text-destructive">Rejected: {r.review_reason}</p>
            )}
            {r.reviewed_by_name && r.status !== "rejected" && (
              <p className="text-xs text-subtle-foreground">Approved by {r.reviewed_by_name}</p>
            )}
          </li>
        );
      })}
    </ol>
  );
}
