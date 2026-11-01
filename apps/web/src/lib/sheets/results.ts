import { useCallback, useSyncExternalStore } from "react";

import type { QueryResult } from "@/lib/data/client";

/** What one visual has to show. */
export interface VisualResult {
  main?: QueryResult;
  /** A KPI's previous period. */
  compare?: QueryResult;
  error?: string;
}

const EMPTY: VisualResult = {};

/**
 * Per-visual query results, outside React state (spec §2.1). Each visual subscribes to its own
 * slot, and a slot only changes when its data does: a result with the same fingerprint as the
 * one shown (a cache hit for an unaffected visual during a cross-filter) is dropped, so that
 * visual doesn't re-render at all.
 */
export class VisualResults {
  private slots = new Map<string, VisualResult>();
  private listeners = new Map<string, Set<() => void>>();

  get(id: string): VisualResult {
    return this.slots.get(id) ?? EMPTY;
  }

  /** Stores one part of a visual's result; returns whether the slot changed. */
  put(id: string, part: "main" | "compare", result: QueryResult): boolean {
    const slot = this.get(id);
    if (slot[part]?.header.fingerprint === result.header.fingerprint && !slot.error) return false;
    this.update(id, { ...slot, [part]: result, error: undefined });
    return true;
  }

  fail(id: string, message: string): void {
    const slot = this.get(id);
    if (slot.error === message) return;
    this.update(id, { ...slot, error: message });
  }

  /** A KPI whose comparison no longer applies (the date filter was cleared). */
  dropCompare(id: string): void {
    const slot = this.get(id);
    if (!slot.compare) return;
    this.update(id, { ...slot, compare: undefined });
  }

  forget(id: string): void {
    this.slots.delete(id);
    this.notify(id);
  }

  subscribe(id: string, listener: () => void): () => void {
    let set = this.listeners.get(id);
    if (!set) {
      set = new Set();
      this.listeners.set(id, set);
    }
    set.add(listener);
    return () => set.delete(listener);
  }

  private update(id: string, slot: VisualResult): void {
    this.slots.set(id, slot);
    this.notify(id);
  }

  private notify(id: string): void {
    for (const listener of this.listeners.get(id) ?? []) listener();
  }
}

export function useVisualResult(results: VisualResults, id: string): VisualResult {
  const subscribe = useCallback((l: () => void) => results.subscribe(id, l), [results, id]);
  const get = useCallback(() => results.get(id), [results, id]);
  return useSyncExternalStore(subscribe, get, () => EMPTY);
}

/** `"<visual id>"` or `"<visual id>:compare"` → id and part. */
export function splitQueryId(queryId: string): { id: string; part: "main" | "compare" } {
  const [id = queryId, suffix] = queryId.split(":");
  return { id, part: suffix === "compare" ? "compare" : "main" };
}
