"use client";

import { createContext, useContext } from "react";
import { useStore } from "zustand";

import type { QueryResult } from "@/lib/data/client";
import type { VisualResults } from "@/lib/sheets/results";
import type { RunTarget } from "@/lib/sheets/runner";
import type { SheetActions, SheetState, SheetStore } from "@/lib/sheets/store";

/** Everything a visual on a sheet needs, without threading props through the grid. */
export interface SheetEnv {
  store: SheetStore;
  results: VisualResults;
  target: RunTarget;
  /** Editors may change the sheet; viewers and link viewers only interact. */
  canEdit: boolean;
  /** CSV/PNG downloads (always for members; for link viewers when the link allows it). */
  canExport: boolean;
  /** The next page of a raw-row table visual. */
  fetchPage: (visualId: string, offset: number) => Promise<QueryResult | null>;
}

const Env = createContext<SheetEnv | null>(null);

export const SheetEnvProvider = Env.Provider;

export function useSheetEnv(): SheetEnv {
  const env = useContext(Env);
  if (!env) throw new Error("useSheetEnv outside a sheet");
  return env;
}

/** A slice of the sheet store; re-renders only when the slice changes. */
export function useSheetState<T>(select: (state: SheetState & SheetActions) => T): T {
  return useStore(useSheetEnv().store, select);
}
