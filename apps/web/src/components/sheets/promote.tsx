"use client";

import type { Measure } from "@lattice/shared-types";
import { ArrowUpRight } from "lucide-react";
import { useState } from "react";

import { usePromote } from "@/lib/knowledge/api";
import { measureLabel } from "@/lib/sheets/charts";

/** "Promote to metric" for a raw aggregation (docs/adr/0006, slice 3.3): names it, writes the
 * formula into a draft metric, and hands back the metric for the sheet to switch to. `uses` is
 * how many visuals on the sheet aggregate the same way (two or more and it's suggested). */
export function Promote({
  projectId,
  tableId,
  measure,
  uses,
  onPromoted,
}: {
  projectId: string;
  tableId: string;
  measure: Measure;
  uses: number;
  onPromoted: (noteId: string) => void;
}) {
  const promote = usePromote(projectId);
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState(measureLabel(measure));
  if (!open) {
    return uses >= 2 ? (
      <p className="flex items-center justify-between gap-2 rounded-md bg-accent px-2.5 py-1.5 text-xs text-accent-foreground">
        <span>
          {measureLabel(measure)} is on {uses} visuals. Make it a metric?
        </span>
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="font-semibold whitespace-nowrap hover:underline"
        >
          Promote
        </button>
      </p>
    ) : (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="flex items-center gap-1 justify-self-start text-xs text-subtle-foreground hover:text-primary"
      >
        <ArrowUpRight className="size-3.5" aria-hidden /> Promote to metric
      </button>
    );
  }
  return (
    <form
      className="grid gap-1.5 rounded-lg border border-border p-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (!title.trim()) return;
        promote.mutate(
          { table_id: tableId, measure, title: title.trim() },
          {
            onSuccess: (r) => {
              setOpen(false);
              onPromoted(r.note_id);
            },
          },
        );
      }}
    >
      <label className="grid gap-1 text-[13px] font-medium">
        Metric name
        <input
          autoFocus
          value={title}
          maxLength={120}
          onChange={(e) => setTitle(e.target.value)}
          className="h-9 rounded-md border border-input bg-card px-2.5 text-sm"
        />
      </label>
      <p className="text-xs text-muted-foreground">
        Starts a draft in Business logic; the visuals use it right away, marked unapproved until
        it&apos;s approved.
      </p>
      {promote.error && (
        <p role="alert" className="text-xs text-destructive">
          {promote.error.message}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="text-xs text-subtle-foreground hover:text-foreground"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={!title.trim() || promote.isPending}
          className="h-7 rounded-md bg-primary px-2.5 text-xs font-semibold text-primary-foreground disabled:opacity-40"
        >
          Create metric
        </button>
      </div>
    </form>
  );
}
