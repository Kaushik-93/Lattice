import type { Dimension, Scalar, TimeBucket, ValueFormat } from "@lattice/shared-types";
import { DataType, type Field, type Table } from "apache-arrow";

import { fiscalQuarterLabel, fiscalYearLabel } from "./fiscal";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** Arrow cells as numbers: int64 arrives as bigint, decimals as float (the gateway casts them). */
export function toNumber(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value === "bigint") return Number(value);
  return null;
}

/** A column's values, decoded once (charts read every row). */
export function columnOf(table: Table, index: number): unknown[] {
  const column = table.getChildAt(index);
  return column ? Array.from(column) : [];
}

export function numbersOf(table: Table, index: number): (number | null)[] {
  return columnOf(table, index).map(toNumber);
}

export function isTemporalField(field: Field | undefined): boolean {
  return field !== undefined && (DataType.isDate(field.type) || DataType.isTimestamp(field.type));
}

/** "$11.7M", "48,210", "2.1%" — the design's KPI and axis style. */
export function formatValue(
  value: number | null,
  format: ValueFormat | undefined,
  { compact = true }: { compact?: boolean } = {},
): string {
  if (value === null) return "—";
  if (format === "percent") {
    return new Intl.NumberFormat("en-US", { style: "percent", maximumFractionDigits: 1 }).format(
      value,
    );
  }
  const big = Math.abs(value) >= 10_000;
  const options: Intl.NumberFormatOptions = {
    notation: compact && big ? "compact" : "standard",
    maximumFractionDigits: compact && big ? 1 : Math.abs(value) < 100 ? 2 : 0,
    minimumFractionDigits:
      format === "currency" && !(compact && big) && Math.abs(value) < 100 ? 2 : 0,
  };
  if (format === "currency") Object.assign(options, { style: "currency", currency: "USD" });
  return new Intl.NumberFormat("en-US", options).format(value);
}

/** "+5.4% vs Q2" style change between two values. */
export function formatDelta(
  current: number | null,
  previous: number | null,
): {
  text: string;
  tone: "up" | "down" | "flat";
} {
  if (current === null || previous === null || previous === 0) return { text: "—", tone: "flat" };
  const change = (current - previous) / Math.abs(previous);
  if (Math.abs(change) < 0.0005) return { text: "Flat", tone: "flat" };
  const text = new Intl.NumberFormat("en-US", {
    style: "percent",
    maximumFractionDigits: 1,
    signDisplay: "always",
  })
    .format(change)
    .replace("-", "−");
  return { text, tone: change > 0 ? "up" : "down" };
}

/** Epoch milliseconds as a label for its time bucket: "2025", "Q3 2025", "Aug 2025". */
export function formatDate(
  ms: number,
  bucket?: TimeBucket | null,
  timestamp = false,
  fiscalStart = 1,
): string {
  const d = new Date(ms);
  const [y, m, day] = [d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()];
  switch (bucket) {
    case "year":
      return String(y);
    case "quarter":
      return `Q${Math.floor(m / 3) + 1} ${y}`;
    case "month":
      return `${MONTHS[m]} ${y}`;
    case "week":
      return `${MONTHS[m]} ${day}, ${y}`;
    case "fiscal_quarter":
      return fiscalQuarterLabel(ms, fiscalStart);
    case "fiscal_year":
      return fiscalYearLabel(ms, fiscalStart);
    default:
      return d
        .toISOString()
        .slice(0, timestamp ? 16 : 10)
        .replace("T", " ");
  }
}

/** A dimension value as a label, aware of the time bucket it was grouped by. */
export function formatDimension(
  value: unknown,
  field: Field | undefined,
  dim?: Dimension,
  fiscalStart = 1,
): string {
  if (value === null || value === undefined) return "(blank)";
  if (field && isTemporalField(field) && typeof value === "number") {
    return formatDate(value, dim?.bucket, DataType.isTimestamp(field.type), fiscalStart);
  }
  if (dim?.bin !== undefined && dim.bin !== null) {
    const lo = toNumber(value);
    if (lo !== null) return `${formatValue(lo, undefined)}–${formatValue(lo + dim.bin, undefined)}`;
  }
  if (typeof value === "bigint") return value.toLocaleString("en-US");
  if (typeof value === "number") return formatValue(value, undefined, { compact: false });
  return String(value);
}

/** A clicked dimension value as a filter scalar (dates as ISO dates, as the gateway expects). */
export function scalarOf(value: unknown, field: Field | undefined): Scalar | null {
  if (value === null || value === undefined) return null;
  if (isTemporalField(field) && typeof value === "number") {
    return new Date(value).toISOString().slice(0, 10);
  }
  if (typeof value === "bigint") return Number(value);
  if (typeof value === "number" || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  return String(value);
}
