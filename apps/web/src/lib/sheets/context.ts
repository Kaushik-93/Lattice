import type {
  Filter,
  FilterValue,
  Scalar,
  Selection,
  SheetContext,
  SheetFilterDef,
} from "@lattice/shared-types";

/** Current values of a sheet's filters by filter id. Missing means "the saved default"; null
 * means "cleared" (unfiltered). */
export type FilterValues = Record<string, FilterValue | null>;

export function currentValue(def: SheetFilterDef, values: FilterValues): FilterValue | null {
  return def.id in values ? (values[def.id] ?? null) : (def.value ?? null);
}

/** The query filter a sheet filter's value stands for, or null when unfiltered. */
export function filterFor(def: SheetFilterDef, value: FilterValue | null): Filter | null {
  if (value === null || !Array.isArray(value) || value.length === 0) return null;
  if (def.kind === "date_range") {
    return value.length === 2 ? { column: def.column, op: "between", value } : null;
  }
  return value.length === 1
    ? { column: def.column, op: "eq", value: value[0] as Scalar }
    : { column: def.column, op: "in", value };
}

/** The state every visual of the sheet is compiled against (the gateway decides which filter
 * applies to which visual by its table's columns). */
export function buildContext(
  defs: SheetFilterDef[],
  values: FilterValues,
  selections: Selection[],
): SheetContext {
  const filters = defs
    .map((def) => filterFor(def, currentValue(def, values)))
    .filter((f): f is Filter => f !== null);
  return { filters, selections };
}

/**
 * Clicking a value on a visual: selects it, or clears the selection when it was already the
 * visual's selection. One selection per visual (a click replaces the previous one).
 */
export function toggleSelection(selections: Selection[], next: Selection): Selection[] {
  const others = selections.filter((s) => s.visual_id !== next.visual_id);
  const current = selections.find((s) => s.visual_id === next.visual_id);
  const same =
    current !== undefined && JSON.stringify(current.filter) === JSON.stringify(next.filter);
  return same ? others : [...others, next];
}

/** The value a visual has selected on `column`, for highlighting it. */
export function selectedValues(selections: Selection[], visualId: string): Scalar[] {
  const s = selections.find((x) => x.visual_id === visualId);
  if (!s?.filter.value) return [];
  return Array.isArray(s.filter.value) ? s.filter.value : [s.filter.value];
}
