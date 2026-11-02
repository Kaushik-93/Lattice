"use client";

import type { ProjectSuggestions, Suggestion, SuggestionKind } from "@lattice/shared-types";
import { useState } from "react";

import { Button } from "@/components/ui/button";
import { useSuggestionAction, useSuggestions } from "@/lib/knowledge/api";

const KIND: Record<SuggestionKind, string> = {
  entity: "Entity",
  binding: "Connect entity",
  relationship: "Relationship",
  dimension: "Dimension",
};

/** Knowledge picked up from data profiles (docs/adr/0006): nothing reaches the graph until
 * someone accepts it, and then only as a draft. */
export function Suggestions({ onOpen }: { onOpen: (noteId: string) => void }) {
  const suggestions = useSuggestions();
  const projects = suggestions.data ?? [];
  const count = projects.reduce((n, p) => n + p.items.length, 0);
  if (!suggestions.data || count === 0) return null;
  return (
    <section aria-labelledby="suggestions-h" className="flex flex-col gap-3">
      <h2 id="suggestions-h" className="flex items-center gap-2 text-[17px] font-semibold">
        Picked up from your data
        <span className="rounded-full bg-accent px-2 py-0.5 text-xs font-semibold text-accent-foreground">
          {count}
        </span>
      </h2>
      <p className="-mt-1 text-[13px] text-muted-foreground">
        From the profiles of connected tables. Accept one to start a draft; nothing is used until
        it&apos;s approved.
      </p>
      {projects.map((p) => (
        <ProjectGroup key={p.project_id} group={p} onOpen={onOpen} />
      ))}
    </section>
  );
}

function ProjectGroup({
  group,
  onOpen,
}: {
  group: ProjectSuggestions;
  onOpen: (noteId: string) => void;
}) {
  const action = useSuggestionAction();
  const [busy, setBusy] = useState(false);
  const acceptAll = async () => {
    setBusy(true);
    try {
      // In order: entities first, so relationships and dimensions find them.
      for (const s of group.items) {
        await action.mutateAsync({ project: group.project_id, key: s.key, accept: true });
      }
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col gap-2" data-testid="suggestion-group">
      <div className="flex items-center justify-between">
        <span className="text-[13px] font-semibold text-secondary-foreground">
          {group.project_name}
        </span>
        <button
          type="button"
          disabled={busy || action.isPending}
          onClick={() => void acceptAll()}
          className="text-xs font-medium text-primary hover:underline disabled:opacity-50"
        >
          Accept all {group.items.length}
        </button>
      </div>
      <ul className="flex flex-col gap-2">
        {group.items.map((s) => (
          <SuggestionCard
            key={s.key}
            suggestion={s}
            disabled={busy || action.isPending}
            onAccept={async () => {
              const accepted = await action.mutateAsync({
                project: group.project_id,
                key: s.key,
                accept: true,
              });
              if (accepted) onOpen(accepted.note_id);
            }}
            onDismiss={() =>
              action.mutate({ project: group.project_id, key: s.key, accept: false })
            }
          />
        ))}
      </ul>
      {action.error && (
        <p role="alert" className="text-sm text-destructive">
          {action.error.message}
        </p>
      )}
    </div>
  );
}

function SuggestionCard({
  suggestion: s,
  disabled,
  onAccept,
  onDismiss,
}: {
  suggestion: Suggestion;
  disabled: boolean;
  onAccept: () => void;
  onDismiss: () => void;
}) {
  return (
    <li
      className="flex flex-col gap-1.5 rounded-xl border border-border bg-card p-3.5"
      data-testid="suggestion"
      aria-label={`${KIND[s.kind]}: ${s.title}`}
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[14px] font-semibold">{s.title}</span>
        <span className="text-[11px] font-semibold tracking-wide whitespace-nowrap text-subtle-foreground uppercase">
          {KIND[s.kind]}
        </span>
      </div>
      <span className="text-xs text-muted-foreground">{s.table}</span>
      {s.evidence.length > 0 && (
        <ul className="list-disc pl-4 text-xs text-secondary-foreground">
          {s.evidence.map((e) => (
            <li key={e}>{e}</li>
          ))}
        </ul>
      )}
      <div className="mt-1 flex gap-2">
        <Button size="sm" disabled={disabled} onClick={onAccept}>
          Accept
        </Button>
        <Button size="sm" variant="ghost" disabled={disabled} onClick={onDismiss}>
          Dismiss
        </Button>
      </div>
    </li>
  );
}
