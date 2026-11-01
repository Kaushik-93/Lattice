import type { Dimension, Measure, VisualKind, VisualSpec } from "@lattice/shared-types";

/** A field in the editor's field list: a governed metric, or one of the table's columns. */
export type Field =
  | { kind: "metric"; id: string; title: string }
  | { kind: "column"; name: string; dataType: string };

/** Where a field can go on a visual (ADR 0007). */
export type Slot = "axis" | "legend" | "values" | "drill";

const NUMERIC =
  /^(TINYINT|SMALLINT|INTEGER|INT|BIGINT|HUGEINT|UTINYINT|USMALLINT|UINTEGER|UBIGINT|FLOAT|REAL|DOUBLE|DECIMAL|NUMERIC)/i;
const TEMPORAL = /^(DATE|TIMESTAMP)/i;

const SPLIT = new Set<VisualKind>([
  "line",
  "area",
  "bar",
  "column",
  "scatter",
  "pivot",
  "heatmap",
  "table",
]);
const SINGLE = new Set<VisualKind>(["kpi", "pie", "donut", "heatmap"]);
const DRILL = new Set<VisualKind>(["bar", "column", "line", "area", "pie", "donut"]);
const NO_VALUES = new Set<VisualKind>(["scatter", "slicer", "histogram"]);

/** The slots a visual of this kind has. */
export function slotsOf(spec: VisualSpec): Slot[] {
  const slots: Slot[] = [];
  if (spec.kind !== "kpi" && !(spec.kind === "table" && !spec.x)) slots.push("axis");
  if (SPLIT.has(spec.kind)) slots.push("legend");
  if (!NO_VALUES.has(spec.kind) && !(spec.kind === "table" && !spec.x)) slots.push("values");
  if (DRILL.has(spec.kind)) slots.push("drill");
  return slots;
}

/** Whether a field fits a slot: metrics only make values; histograms bin numbers. */
export function accepts(spec: VisualSpec, slot: Slot, field: Field): boolean {
  if (!slotsOf(spec).includes(slot)) return false;
  if (field.kind === "metric") return slot === "values";
  if (slot === "axis" && spec.kind === "histogram") return NUMERIC.test(field.dataType);
  return true;
}

const metric = (id: string): Measure => ({ column: null, agg: "sum", metric: id });

/** The visual with `field` dropped on `slot`. A visual charts metrics or raw columns, not both
 * (ADR 0006), so a value of the other sort replaces what's there. */
export function placeField(
  spec: VisualSpec,
  slot: Slot,
  field: Field,
  binWidth: (column: string) => number = () => 1,
): VisualSpec {
  if (!accepts(spec, slot, field)) return spec;
  if (field.kind === "metric") {
    const m = metric(field.id);
    const governed = spec.measures.every((x) => x.metric);
    if (SINGLE.has(spec.kind) || !governed) return { ...spec, measures: [m] };
    if (spec.measures.some((x) => x.metric === field.id)) return spec;
    return { ...spec, measures: [...spec.measures, m].slice(0, 6) };
  }
  const column = field.name;
  const dim = (): Dimension => ({
    column,
    bucket: TEMPORAL.test(field.dataType) ? "month" : null,
  });
  switch (slot) {
    case "axis":
      if (spec.kind === "histogram") {
        return { ...spec, x: { column, bucket: null, bin: binWidth(column) } };
      }
      return { ...spec, x: dim(), drill: [] };
    case "legend":
      return { ...spec, series: dim() };
    case "drill":
      if (spec.x?.column === column || spec.drill.some((d) => d.column === column)) return spec;
      return { ...spec, drill: [...spec.drill, { column, bucket: null }] };
    case "values": {
      const m: Measure = NUMERIC.test(field.dataType)
        ? { column, agg: "sum" }
        : { column, agg: "count_distinct" };
      const raw = spec.measures.every((x) => !x.metric);
      if (SINGLE.has(spec.kind) || !raw) return { ...spec, measures: [m] };
      return { ...spec, measures: [...spec.measures, m].slice(0, 6) };
    }
  }
}

const MIME = { metric: "application/x-lattice-metric", column: "application/x-lattice-column" };

/** Puts a field on a drag. Its sort is in the MIME type, so drop targets can accept or refuse
 * it while it's still over them (the data itself is only readable on drop). */
export function startDrag(e: { dataTransfer: DataTransfer }, field: Field) {
  e.dataTransfer.setData(MIME[field.kind], JSON.stringify(field));
  e.dataTransfer.effectAllowed = "copy";
}

export function draggedKind(types: readonly string[]): Field["kind"] | null {
  if (types.includes(MIME.metric)) return "metric";
  if (types.includes(MIME.column)) return "column";
  return null;
}

export function droppedField(data: DataTransfer): Field | null {
  const raw = data.getData(MIME.metric) || data.getData(MIME.column);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as Field;
  } catch {
    return null;
  }
}
