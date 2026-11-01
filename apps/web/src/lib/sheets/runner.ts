"use client";

import type { SharedVisualRequest, VisualRequest } from "@lattice/shared-types";
import { useEffect } from "react";

import { dataClient, type BatchOutcome, type QueryResult } from "@/lib/data/client";
import { perf } from "@/lib/perf/store";
import { buildContext } from "@/lib/sheets/context";
import { splitQueryId, type VisualResults } from "@/lib/sheets/results";
import type { SheetState, SheetStore } from "@/lib/sheets/store";

export type RunTarget = { kind: "member"; projectId: string } | { kind: "shared"; token: string };

export interface Settled {
  /** Interaction → every result rendered. */
  ms: number;
  changed: number;
  visuals: number;
}

/** What determines a visual's data: its spec minus presentation, plus its drill path. */
function requestsOf(state: SheetState): VisualRequest[] {
  return state.doc.visuals.map((v) => ({
    id: v.id,
    spec: v.spec,
    drill: state.drill[v.id] ?? [],
  }));
}

function queryKey(state: SheetState, requests: VisualRequest[]): string {
  const context = buildContext(state.doc.filters, state.filterValues, state.selections);
  // Titles and options like number formats don't change the data.
  const specs = requests.map((r) => [
    r.id,
    { ...r.spec, title: "", options: { ...r.spec.options, format: undefined } },
    r.drill,
  ]);
  return JSON.stringify([context, specs]);
}

/**
 * Runs the sheet's visuals as one batch whenever anything that feeds them changes — filters,
 * selections, drill paths, specs — and streams each result into `results` as it arrives
 * (spec §3 rules 6–7: the newest state cancels older work end to end). Subscribes to the store
 * directly, so the page component doesn't re-render on every interaction.
 */
export function useSheetRunner(
  target: RunTarget,
  store: SheetStore,
  results: VisualResults,
  onSettled?: (settled: Settled) => void,
): void {
  const targetKey = target.kind === "member" ? target.projectId : target.token;
  useEffect(() => {
    const client = dataClient();
    let last = "";
    let alive = true;

    const run = (state: SheetState) => {
      const requests = requestsOf(state);
      const key = queryKey(state, requests);
      if (key === last) return;
      last = key;
      if (requests.length === 0) return;

      const startedAt = state.interactionAt || performance.now();
      const context = buildContext(state.doc.filters, state.filterValues, state.selections);
      let changed = 0;
      const onResult = (result: QueryResult) => {
        const { id, part } = splitQueryId(result.header.query_id);
        if (results.put(id, part, result)) changed += 1;
      };
      const batch: Promise<BatchOutcome> =
        target.kind === "member"
          ? client.runSheet(target.projectId, context, requests, onResult)
          : client.runShared(
              target.token,
              context,
              requests.map(({ id, drill }): SharedVisualRequest => ({ id, drill })),
              onResult,
            );
      void batch.then((outcome) => {
        if (!alive || outcome.superseded) return;
        for (const error of outcome.errors) {
          const ids =
            error.queryId === "*" ? requests.map((r) => r.id) : [splitQueryId(error.queryId).id];
          for (const id of ids) results.fail(id, error.message);
        }
        for (const r of requests) {
          if (r.spec.kind === "kpi" && !outcome.results.has(`${r.id}:compare`)) {
            results.dropCompare(r.id);
          }
        }
        // After the browser has painted the last result.
        requestAnimationFrame(() => {
          const settled = { ms: performance.now() - startedAt, changed, visuals: requests.length };
          perf.sheets.push({ ...settled, at: performance.now() });
          onSettled?.(settled);
        });
      });
    };

    run(store.getState());
    const unsubscribe = store.subscribe(run);
    return () => {
      alive = false;
      unsubscribe();
    };
    // `target` is identified by targetKey; onSettled is read once per run.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [store, results, targetKey]);
}
