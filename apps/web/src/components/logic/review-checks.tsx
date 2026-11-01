"use client";

import type { CaseResult, NoteDetail } from "@lattice/shared-types";
import { CircleCheck, CircleX, RotateCw } from "lucide-react";

import { Skeleton } from "@/components/ui/skeleton";
import { useCaseRun, useImpact } from "@/lib/knowledge/api";
import { casesOf } from "@/lib/knowledge/definition";
import { cn } from "@/lib/utils";

const number = (n: number | null) =>
  n === null ? "nothing" : n.toLocaleString("en-US", { maximumFractionDigits: 6 });

/** Whether a note's example cases allow approving it: `null` while they run. */
export function useCasesPass(note: NoteDetail): boolean | null {
  const revision = note.open ?? note.live;
  const has = casesOf(revision?.definition as Record<string, unknown> | null).length > 0;
  const run = useCaseRun(note.id, has && revision ? revision.version : null);
  if (!has) return true;
  if (!run.data) return null;
  return run.data.results.every((r) => r.passed);
}

/** The example cases of the change under way (or the live version), run on their own rows. */
export function ExampleCases({ note }: { note: NoteDetail }) {
  const revision = note.open ?? note.live;
  const cases = casesOf(revision?.definition as Record<string, unknown> | null);
  const run = useCaseRun(note.id, cases.length > 0 && revision ? revision.version : null);
  if (note.kind !== "metric" && note.kind !== "rule") return null;
  return (
    <section aria-label="Example cases" className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <span className="text-[13px] font-semibold">Example cases</span>
        {cases.length > 0 && (
          <button
            type="button"
            onClick={() => void run.refetch()}
            disabled={run.isFetching}
            className="flex items-center gap-1 text-xs text-subtle-foreground hover:text-foreground disabled:opacity-50"
          >
            <RotateCw className={cn("size-3", run.isFetching && "animate-spin")} aria-hidden />
            Run again
          </button>
        )}
      </div>
      {cases.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">
          None yet. A definition with example cases can only be approved while they pass.
        </p>
      ) : run.error ? (
        <p role="alert" className="text-[13px] text-destructive">
          {run.error.message}
        </p>
      ) : !run.data ? (
        <Skeleton className="h-9 w-full" />
      ) : (
        <ul className="flex flex-col gap-1">
          {run.data.results.map((r, i) => (
            <CaseRow key={i} result={r} />
          ))}
        </ul>
      )}
    </section>
  );
}

function CaseRow({ result }: { result: CaseResult }) {
  return (
    <li
      className={cn(
        "flex items-start gap-2 rounded-lg px-3 py-2 text-[13px]",
        result.passed ? "bg-success-surface" : "bg-destructive-surface",
      )}
    >
      {result.passed ? (
        <CircleCheck className="mt-0.5 size-4 flex-none text-success" aria-hidden />
      ) : (
        <CircleX className="mt-0.5 size-4 flex-none text-destructive" aria-hidden />
      )}
      <div className="flex min-w-0 flex-col">
        <span className="font-medium">
          {result.name}
          <span className="sr-only">{result.passed ? " passes" : " fails"}</span>
        </span>
        <span className="text-xs text-secondary-foreground">
          {result.error
            ? result.error
            : result.passed
              ? `Gives ${number(result.actual)}, as expected.`
              : `Expected ${number(result.expected)}, got ${number(result.actual)}.`}
        </span>
      </div>
    </li>
  );
}

/** What approving the pending change touches: definitions using it and the visuals charting
 * them. */
export function Impact({ note }: { note: NoteDetail }) {
  const impact = useImpact(note.id, true);
  if (!impact.data) return impact.isPending ? <Skeleton className="h-9 w-full" /> : null;
  const { dependents, visuals, hidden_visuals } = impact.data;
  const sheets = new Set(visuals.map((v) => v.sheet_id)).size;
  return (
    <section aria-label="Impact" className="flex flex-col gap-1.5">
      <span className="text-[13px] font-semibold">Approving this changes</span>
      {dependents.length === 0 && visuals.length === 0 && hidden_visuals === 0 ? (
        <p className="text-[13px] text-muted-foreground">Nothing else uses it yet.</p>
      ) : (
        <div className="flex flex-col gap-1.5 text-[13px]">
          {dependents.length > 0 && (
            <p>
              {dependents.length === 1 ? "1 definition" : `${dependents.length} definitions`} that
              use it: {dependents.map((d) => d.title).join(", ")}
            </p>
          )}
          {visuals.length > 0 && (
            <div className="flex flex-col gap-1">
              <p>
                {visuals.length === 1 ? "1 visual" : `${visuals.length} visuals`} on{" "}
                {sheets === 1 ? "1 sheet" : `${sheets} sheets`}:
              </p>
              <ul className="flex flex-col gap-0.5 text-xs text-secondary-foreground">
                {visuals.map((v) => (
                  <li key={v.visual_id}>
                    {v.title} · {v.sheet_name}{" "}
                    <span className="text-subtle-foreground">({v.project_name})</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {hidden_visuals > 0 && (
            <p className="text-xs text-muted-foreground">
              And {hidden_visuals} in projects you can&apos;t open.
            </p>
          )}
        </div>
      )}
    </section>
  );
}
