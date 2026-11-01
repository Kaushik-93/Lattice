import type {
  FilterValue,
  Scalar,
  Selection,
  Sheet,
  SheetFilterDef,
  Visual,
} from "@lattice/shared-types";
import { createStore, type StoreApi } from "zustand/vanilla";

import type { FilterValues } from "@/lib/sheets/context";
import { toggleSelection } from "@/lib/sheets/context";

/** The saved, undoable part of a sheet. */
export interface SheetDoc {
  name: string;
  filters: SheetFilterDef[];
  visuals: Visual[];
}

/**
 * An undoable edit (spec §8: command pattern). Commands carry the document before and after,
 * so undo and redo are exact whatever the edit was; consecutive edits with the same `merge`
 * key (typing a title, nudging a slider) collapse into one command.
 */
export interface Command {
  label: string;
  before: SheetDoc;
  after: SheetDoc;
  merge?: string;
  at: number;
}

export type SaveState = "saved" | "dirty" | "saving" | "conflict" | "error";

export interface SheetState {
  projectId: string;
  sheetId: string;
  doc: SheetDoc;
  /** Server version the doc is based on. */
  version: number;
  past: Command[];
  future: Command[];
  saveState: SaveState;
  saveError: string | null;

  editing: boolean;
  selectedVisual: string | null;

  /** Viewer state: not saved, not undoable. */
  filterValues: FilterValues;
  selections: Selection[];
  drill: Record<string, Scalar[]>;
  /** `performance.now()` of the last change that re-queries the sheet (perf overlay, tests). */
  interactionAt: number;
}

export interface SheetActions {
  /** Applies an edit as one undoable command. */
  edit(label: string, recipe: (doc: SheetDoc) => SheetDoc, merge?: string): void;
  undo(): void;
  redo(): void;
  setEditing(editing: boolean): void;
  selectVisual(id: string | null): void;
  setFilterValue(id: string, value: FilterValue | null): void;
  select(selection: Selection): void;
  /** Replaces (or with null, clears) one visual's selection: slicers pick several values. */
  setSelection(visualId: string, selection: Selection | null): void;
  clearSelections(): void;
  drillDown(visualId: string, value: Scalar): void;
  drillTo(visualId: string, depth: number): void;
  saving(): void;
  saved(version: number): void;
  failed(state: "conflict" | "error", message: string): void;
}

export type SheetStore = StoreApi<SheetState & SheetActions>;

/** Commands kept per sheet. */
const HISTORY_LIMIT = 100;
/** Edits with the same merge key closer together than this become one command. */
const MERGE_WINDOW_MS = 1500;

export function docOf(sheet: Pick<Sheet, "name" | "filters" | "visuals">): SheetDoc {
  return { name: sheet.name, filters: sheet.filters, visuals: sheet.visuals };
}

interface PersistedHistory {
  version: number;
  past: Command[];
  future: Command[];
}

const historyKey = (sheetId: string) => `lattice:sheet-history:${sheetId}`;

/** Undo history survives reloads within the browser session (spec §8), as long as nobody has
 * saved the sheet since. */
function loadHistory(sheet: Sheet): PersistedHistory | null {
  try {
    const raw = sessionStorage.getItem(historyKey(sheet.id));
    const parsed = raw ? (JSON.parse(raw) as PersistedHistory) : null;
    return parsed?.version === sheet.version ? parsed : null;
  } catch {
    return null;
  }
}

function saveHistory(sheetId: string, history: PersistedHistory): void {
  try {
    sessionStorage.setItem(historyKey(sheetId), JSON.stringify(history));
  } catch {
    // Storage full or unavailable: history just won't survive a reload.
  }
}

const EDIT_MODE = "lattice:edit-mode";

/** Edit mode is global (ADR 0007): on for every sheet in this tab, across reloads, until turned
 * off. */
function editModeOn(): boolean {
  try {
    return sessionStorage.getItem(EDIT_MODE) === "on";
  } catch {
    return false;
  }
}

function rememberEditMode(on: boolean) {
  try {
    if (on) sessionStorage.setItem(EDIT_MODE, "on");
    else sessionStorage.removeItem(EDIT_MODE);
  } catch {
    // Unavailable storage: edit mode just won't carry over.
  }
}

export function createSheetStore(projectId: string, sheet: Sheet, canEdit: boolean): SheetStore {
  const restored = canEdit ? loadHistory(sheet) : null;
  const store = createStore<SheetState & SheetActions>()((set, get) => {
    const touched = () => ({ interactionAt: performance.now() });
    return {
      projectId,
      sheetId: sheet.id,
      doc: docOf(sheet),
      version: sheet.version,
      past: restored?.past ?? [],
      future: restored?.future ?? [],
      saveState: "saved",
      saveError: null,
      editing: canEdit && editModeOn(),
      selectedVisual: null,
      filterValues: {},
      selections: [],
      drill: {},
      interactionAt: 0,

      edit(label, recipe, merge) {
        const { doc, past } = get();
        const after = recipe(doc);
        if (after === doc) return;
        const now = performance.now();
        const last = past.at(-1);
        const command: Command =
          merge && last?.merge === merge && now - last.at < MERGE_WINDOW_MS
            ? { ...last, after, at: now }
            : { label, before: doc, after, merge, at: now };
        const history =
          command.before === last?.before && last?.merge === merge ? past.slice(0, -1) : past;
        set({
          doc: after,
          past: [...history, command].slice(-HISTORY_LIMIT),
          future: [],
          saveState: "dirty",
          ...touched(),
        });
      },
      undo() {
        const { past, future } = get();
        const command = past.at(-1);
        if (!command) return;
        set({
          doc: command.before,
          past: past.slice(0, -1),
          future: [command, ...future],
          saveState: "dirty",
          ...touched(),
        });
      },
      redo() {
        const { past, future } = get();
        const command = future[0];
        if (!command) return;
        set({
          doc: command.after,
          past: [...past, command],
          future: future.slice(1),
          saveState: "dirty",
          ...touched(),
        });
      },
      setEditing(editing) {
        rememberEditMode(editing);
        set({ editing, selectedVisual: editing ? get().selectedVisual : null });
      },
      selectVisual(id) {
        set({ selectedVisual: id });
      },
      setFilterValue(id, value) {
        set({ filterValues: { ...get().filterValues, [id]: value }, ...touched() });
      },
      select(selection) {
        set({ selections: toggleSelection(get().selections, selection), ...touched() });
      },
      setSelection(visualId, selection) {
        const others = get().selections.filter((s) => s.visual_id !== visualId);
        set({ selections: selection ? [...others, selection] : others, ...touched() });
      },
      clearSelections() {
        if (get().selections.length === 0) return;
        set({ selections: [], ...touched() });
      },
      drillDown(visualId, value) {
        const drill = get().drill;
        // Drilling replaces the visual's own selection: its axis now shows the next level.
        set({
          drill: { ...drill, [visualId]: [...(drill[visualId] ?? []), value] },
          selections: get().selections.filter((s) => s.visual_id !== visualId),
          ...touched(),
        });
      },
      drillTo(visualId, depth) {
        const drill = get().drill;
        set({
          drill: { ...drill, [visualId]: (drill[visualId] ?? []).slice(0, depth) },
          ...touched(),
        });
      },
      saving() {
        set({ saveState: "saving", saveError: null });
      },
      saved(version) {
        // Edits made while the save was in flight keep the sheet dirty.
        set({ version, saveState: get().saveState === "saving" ? "saved" : get().saveState });
      },
      failed(state, message) {
        set({ saveState: state, saveError: message });
      },
    };
  });
  if (canEdit) {
    store.subscribe((s, prev) => {
      if (s.past !== prev.past || s.future !== prev.future || s.version !== prev.version) {
        saveHistory(s.sheetId, { version: s.version, past: s.past, future: s.future });
      }
    });
  }
  return store;
}

/** Visual ids are made in the browser so an unsaved visual already has its final id. */
export function newId(): string {
  return crypto.randomUUID();
}
