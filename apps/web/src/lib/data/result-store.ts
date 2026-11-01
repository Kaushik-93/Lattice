import { useState, useSyncExternalStore } from "react";

/**
 * Holds query results outside React state (spec §2.1: Arrow tables live in refs or a
 * non-reactive store). Components subscribe and re-render only when their slot changes.
 */
export class ResultStore<T> {
  private value: T | null = null;
  private listeners = new Set<() => void>();
  /** `performance.now()` of the last update, for measuring render time. */
  updatedAt = 0;

  get = (): T | null => this.value;

  set(value: T | null): void {
    this.value = value;
    this.updatedAt = performance.now();
    for (const listener of this.listeners) listener();
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
}

/** A component-owned store plus its current value. */
export function useResultStore<T>(): [ResultStore<T>, T | null] {
  const [store] = useState(() => new ResultStore<T>());
  const value = useSyncExternalStore(store.subscribe, store.get, () => null);
  return [store, value];
}
