"use client";

import type { QueueItem } from "@lattice/shared-types";

import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useNoteAction, useReviewQueue } from "@/lib/knowledge/api";
import { KIND_LABEL } from "@/lib/knowledge/style";

import { ReviewButtons } from "./note-panel";

function heading(item: QueueItem): string {
  const kind = KIND_LABEL[item.note.kind].toLowerCase();
  if (item.live) return `Change to the ${kind} “${item.live.title}”`;
  return `New ${kind}: ${item.revision.title}`;
}

function summary(item: QueueItem): string {
  const text = item.revision.change_note ?? item.revision.body;
  const plain = text.replace(
    /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g,
    (_, t: string, a?: string) => a ?? t,
  );
  return plain.length > 220 ? `${plain.slice(0, 217)}…` : plain;
}

function source(item: QueueItem): string {
  const s = (item.revision.provenance as { source?: string }).source;
  if (s === "demo") return "Demo content";
  if (item.revision.author === "ai") return "From the agent";
  return item.revision.author_name ? `By ${item.revision.author_name}` : "By a teammate";
}

/**
 * Knowledge waiting on someone (design screen 05's "Questions from the agent"). From phase 4
 * the agent's proposals and questions arrive here too; today they come from people and the demo.
 */
export function ReviewQueue({
  onOpen,
  onEdit,
}: {
  onOpen: (noteId: string) => void;
  onEdit: (noteId: string) => void;
}) {
  const queue = useReviewQueue();
  const pending = (queue.data ?? []).filter((i) => i.revision.status === "pending");
  const drafts = (queue.data ?? []).filter((i) => i.revision.status === "draft");
  return (
    <section aria-labelledby="queue-h" className="flex flex-col gap-3">
      <h2 id="queue-h" className="flex items-center gap-2 text-[17px] font-semibold">
        Waiting for review
        <span className="rounded-full bg-warning-surface px-2 py-0.5 text-xs font-semibold text-warning">
          {pending.length} open
        </span>
      </h2>
      {queue.isPending && <Skeleton className="h-40 w-full rounded-xl" />}
      {queue.data && pending.length === 0 && drafts.length === 0 && (
        <p className="rounded-xl border border-dashed border-input bg-card p-5 text-sm text-muted-foreground">
          Nothing waiting. Proposed notes and changes appear here for their owners to approve.
        </p>
      )}
      {pending.map((item) => (
        <QueueCard key={item.revision.id} item={item} onOpen={onOpen} onEdit={onEdit} />
      ))}
      {drafts.length > 0 && (
        <>
          <h3 className="mt-2 text-[13px] font-semibold text-secondary-foreground">Your drafts</h3>
          {drafts.map((item) => (
            <QueueCard key={item.revision.id} item={item} onOpen={onOpen} onEdit={onEdit} />
          ))}
        </>
      )}
    </section>
  );
}

function QueueCard({
  item,
  onOpen,
  onEdit,
}: {
  item: QueueItem;
  onOpen: (id: string) => void;
  onEdit: (id: string) => void;
}) {
  const action = useNoteAction(item.note.id);
  const draft = item.revision.status === "draft";
  return (
    <article
      className="flex flex-col gap-3 rounded-xl border border-border bg-card p-5"
      data-testid="queue-item"
    >
      <button
        type="button"
        className="text-left text-[15px] font-semibold hover:text-primary"
        onClick={() => onOpen(item.note.id)}
      >
        {heading(item)}
      </button>
      <p className="text-sm text-muted-foreground">{summary(item)}</p>
      <div className="flex flex-wrap gap-1.5 text-xs text-secondary-foreground">
        <span className="rounded-md bg-secondary px-2 py-1">{source(item)}</span>
        <span className="rounded-md bg-secondary px-2 py-1">Version {item.revision.version}</span>
        {item.live && (
          <span className="rounded-md bg-secondary px-2 py-1">
            Replaces version {item.live.version}
          </span>
        )}
      </div>
      <div className="flex flex-wrap gap-2">
        {draft ? (
          <>
            <Button onClick={() => action.mutate({ kind: "submit" })} disabled={action.isPending}>
              Submit for review
            </Button>
            <Button variant="outline" onClick={() => onEdit(item.note.id)}>
              Edit
            </Button>
            <Button
              variant="ghost"
              onClick={() => action.mutate({ kind: "discard" })}
              disabled={action.isPending}
            >
              Discard
            </Button>
          </>
        ) : item.can_review ? (
          <>
            <ReviewButtons
              noteId={item.note.id}
              pending={action.isPending}
              onApprove={() => action.mutate({ kind: "approve" })}
              onReject={(reason) => action.mutate({ kind: "reject", reason })}
            />
            <Button variant="outline" onClick={() => onEdit(item.note.id)}>
              Edit first
            </Button>
          </>
        ) : (
          <span className="text-sm text-muted-foreground">Waiting for its owner</span>
        )}
      </div>
      {action.error && (
        <p role="alert" className="text-sm text-destructive">
          {action.error.message}
        </p>
      )}
    </article>
  );
}
