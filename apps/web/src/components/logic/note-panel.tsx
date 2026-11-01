"use client";

import type { NoteDetail, NoteRef, Revision } from "@lattice/shared-types";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useNote, useNoteAction } from "@/lib/knowledge/api";
import { definitionLines } from "@/lib/knowledge/definition";
import { NoteBody } from "@/lib/knowledge/markdown";
import { COMPUTABLE, KIND_LABEL, STATE_PILL } from "@/lib/knowledge/style";
import { cn } from "@/lib/utils";

import { ExampleCases, Impact, useCasesPass } from "./review-checks";

function provenanceLabel(r: Revision): string {
  const source = (r.provenance as { source?: string }).source;
  if (source === "demo") return "Demo content";
  if (r.author === "ai") return "Proposed by the agent";
  if (source === "profile") return "Found while profiling a source";
  return r.author_name ? `Written by ${r.author_name}` : "Written by a person";
}

/** The selected note (design screen 05's right card). */
export function NotePanel({
  id,
  onOpenLink,
  onEdit,
}: {
  id: string;
  onOpenLink: (title: string, from: NoteDetail) => void;
  onEdit: (note: NoteDetail) => void;
}) {
  const note = useNote(id);
  if (note.isPending) {
    return (
      <div className="flex flex-col gap-3 p-6" aria-busy="true">
        <Skeleton className="h-4 w-20" />
        <Skeleton className="h-7 w-48" />
        <Skeleton className="h-20 w-full" />
      </div>
    );
  }
  if (!note.data)
    return <p className="p-6 text-sm text-muted-foreground">This note isn&apos;t available.</p>;
  return <NoteCard note={note.data} onOpenLink={onOpenLink} onEdit={onEdit} />;
}

function NoteCard({
  note,
  onOpenLink,
  onEdit,
}: {
  note: NoteDetail;
  onOpenLink: (title: string, from: NoteDetail) => void;
  onEdit: (note: NoteDetail) => void;
}) {
  const action = useNoteAction(note.id);
  const shown = note.live ?? note.open ?? note.history[0];
  const pill = STATE_PILL[note.state];
  const definition = definitionLines(shown?.definition as Record<string, unknown> | null, {
    cases: false,
  });
  const casesPass = useCasesPass(note);
  const linked = new Map(note.links.map((l) => [l.title.toLowerCase(), l.note]));
  const connected: NoteRef[] = [
    ...note.links.flatMap((l) => (l.note ? [l.note] : [])),
    ...note.backlinks,
  ].filter((n, i, all) => all.findIndex((m) => m.id === n.id) === i);
  const pending = note.open?.status === "pending";

  return (
    <div className="flex h-full flex-col gap-4 p-6" data-testid="note-panel">
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-col gap-1">
          <span className="text-xs font-semibold tracking-wide text-subtle-foreground uppercase">
            {KIND_LABEL[note.kind]}
            {note.project_id ? " · this project only" : ""}
          </span>
          <h2 className="text-xl font-semibold">{shown?.title}</h2>
        </div>
        <span
          className={cn(
            "rounded-full px-2.5 py-1 text-xs font-medium whitespace-nowrap",
            pill.className,
          )}
        >
          {pill.label}
        </span>
      </div>
      {shown && (
        <NoteBody
          body={shown.body || "*No description yet.*"}
          target={(title) => {
            const n = linked.get(title.toLowerCase());
            return { resolved: !!n, tentative: n?.state === "draft" || n?.state === "pending" };
          }}
          onLink={(title) => onOpenLink(title, note)}
        />
      )}
      {COMPUTABLE.includes(note.kind) && definition.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <span className="text-[13px] font-semibold">
            {note.kind === "metric" ? "Formula" : "Definition"}
          </span>
          <pre className="overflow-x-auto rounded-[10px] bg-brand-ink px-4 py-3 font-mono text-[13px] leading-relaxed whitespace-pre-wrap text-white">
            {definition.join("\n")}
          </pre>
        </div>
      )}
      <ExampleCases note={note} />
      {pending && note.can.review && <Impact note={note} />}
      {connected.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <span className="text-[13px] font-semibold">Connected to</span>
          <div className="flex flex-wrap gap-1.5">
            {connected.map((n) => (
              <button
                key={n.id}
                type="button"
                onClick={() => onOpenLink(n.title, note)}
                className="rounded-md bg-secondary px-2.5 py-1 text-xs text-secondary-foreground hover:bg-accent hover:text-accent-foreground"
              >
                {n.title}
              </button>
            ))}
          </div>
        </div>
      )}
      {note.open && note.live && (
        <p className="rounded-[10px] bg-warning-surface px-3 py-2 text-[13px] text-warning">
          Version {note.open.version} is{" "}
          {note.open.status === "pending" ? "waiting for review" : "being drafted"}
          {note.open.change_note ? `: ${note.open.change_note.replace(/[.\s]+$/, "")}` : ""}.
          Version {note.live.version} stays live until then.
        </p>
      )}
      <div className="mt-auto flex flex-col gap-3 border-t border-border-subtle pt-3">
        <div className="flex flex-wrap justify-between gap-2 text-[13px] text-subtle-foreground">
          <span>{shown ? provenanceLabel(shown) : ""}</span>
          <span>
            Owner: {note.owner.team ?? note.owner.name ?? "nobody"}
            {note.owner.team && note.owner.name ? ` (${note.owner.name})` : ""}
          </span>
        </div>
        <div className="flex flex-wrap gap-2">
          {note.can.edit && (
            <Button variant="outline" className="flex-1" onClick={() => onEdit(note)}>
              Open in editor
            </Button>
          )}
          {pending && note.can.review && (
            <ReviewButtons
              noteId={note.id}
              pending={action.isPending}
              blocked={
                casesPass === false
                  ? "Its example cases must pass first."
                  : casesPass === null
                    ? "Running its example cases…"
                    : undefined
              }
              onApprove={() => action.mutate({ kind: "approve" })}
              onReject={(reason) => action.mutate({ kind: "reject", reason })}
            />
          )}
        </div>
        {action.error && (
          <p role="alert" className="text-sm text-destructive">
            {action.error.message}
          </p>
        )}
      </div>
    </div>
  );
}

/** Approve, or "Not right" with a reason (kept, and later fed back to the agent). */
export function ReviewButtons({
  noteId,
  pending,
  blocked,
  onApprove,
  onReject,
}: {
  noteId: string;
  pending: boolean;
  /** Why it can't be approved yet, if it can't. */
  blocked?: string;
  onApprove: () => void;
  onReject: (reason: string) => void;
}) {
  const [rejecting, setRejecting] = useState(false);
  const [reason, setReason] = useState("");
  if (rejecting) {
    return (
      <form
        className="flex w-full flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (reason.trim()) onReject(reason.trim());
        }}
      >
        <label htmlFor={`reason-${noteId}`} className="text-[13px] font-medium">
          What&apos;s not right?
        </label>
        <textarea
          id={`reason-${noteId}`}
          autoFocus
          rows={2}
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          className="rounded-md border border-input px-3 py-2 text-sm"
          placeholder="e.g. Refunds are already excluded by status."
        />
        <div className="flex gap-2">
          <Button type="submit" variant="outline" disabled={!reason.trim() || pending}>
            Reject
          </Button>
          <Button type="button" variant="ghost" onClick={() => setRejecting(false)}>
            Cancel
          </Button>
        </div>
      </form>
    );
  }
  return (
    <>
      <Button onClick={onApprove} disabled={pending || !!blocked} title={blocked}>
        Approve
      </Button>
      <Button variant="ghost" onClick={() => setRejecting(true)}>
        Not right
      </Button>
    </>
  );
}
