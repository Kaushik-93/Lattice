import type { Dimension, Filter, Scalar, TimeBucket, VisualSpec } from "@lattice/shared-types";

const FINER: Record<TimeBucket, TimeBucket[]> = {
  year: ["quarter", "month", "day"],
  quarter: ["month", "day"],
  month: ["day"],
  week: ["day"],
  day: [],
};

/** `x` and the levels a double-click drills into (the gateway's `drill_levels`). */
export function drillLevels(spec: VisualSpec): Dimension[] {
  if (!spec.x) return [];
  if (spec.drill.length > 0) return [spec.x, ...spec.drill];
  const bucket = spec.x.bucket;
  if (!bucket) return [spec.x];
  return [spec.x, ...FINER[bucket].map((b) => ({ column: spec.x!.column, bucket: b }))];
}

/** The dimension on the axis at this drill depth. */
export function currentLevel(spec: VisualSpec, depth: number): Dimension | undefined {
  return drillLevels(spec)[depth];
}

export function canDrill(spec: VisualSpec, depth: number): boolean {
  return depth < drillLevels(spec).length - 1;
}

function addDays(iso: string, days: number): string {
  const d = new Date(`${iso.slice(0, 10)}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

function addMonths(iso: string, months: number): string {
  const d = new Date(`${iso.slice(0, 10)}T00:00:00Z`);
  d.setUTCMonth(d.getUTCMonth() + months);
  return d.toISOString().slice(0, 10);
}

/** Last day of the bucket starting at `start`. */
export function bucketEnd(start: string, bucket: TimeBucket): string {
  switch (bucket) {
    case "year":
      return addDays(addMonths(start, 12), -1);
    case "quarter":
      return addDays(addMonths(start, 3), -1);
    case "month":
      return addDays(addMonths(start, 1), -1);
    case "week":
      return addDays(start, 6);
    case "day":
      return start.slice(0, 10);
  }
}

/** The filter that keeps only the clicked category: equality, or the bucket's / bin's range. */
export function selectionFilter(level: Dimension, value: Scalar): Filter {
  if (level.bucket && typeof value === "string") {
    return { column: level.column, op: "between", value: [value, bucketEnd(value, level.bucket)] };
  }
  if (level.bin !== undefined && level.bin !== null && typeof value === "number") {
    return { column: level.column, op: "between", value: [value, value + level.bin] };
  }
  return { column: level.column, op: "eq", value };
}

/** Slicers pick several values: toggles `value` in the slicer's `in` list (null when empty). */
export function toggleSlicerValue(
  current: Filter | undefined,
  column: string,
  value: Scalar,
): Filter | null {
  const values =
    current?.value === undefined || current?.value === null
      ? []
      : Array.isArray(current.value)
        ? current.value
        : [current.value];
  const next = values.includes(value) ? values.filter((v) => v !== value) : [...values, value];
  return next.length === 0 ? null : { column, op: "in", value: next };
}
