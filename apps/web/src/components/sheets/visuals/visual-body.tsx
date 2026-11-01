"use client";

import type { Scalar, Visual } from "@lattice/shared-types";
import type { Field, Table } from "apache-arrow";
import type { EChartsOption } from "echarts";
import { memo, useEffect, useMemo, useState } from "react";

import { compute } from "@/lib/compute/client";
import {
  foldSlices,
  heatmapOption,
  histogramOption,
  measureLabel,
  pickRows,
  pieOption,
  shape,
  shapedOption,
} from "@/lib/sheets/charts";
import type { VisualResult } from "@/lib/sheets/results";
import {
  columnOf,
  formatDelta,
  formatDimension,
  formatValue,
  scalarOf,
  toNumber,
} from "@/lib/sheets/values";
import { cn } from "@/lib/utils";

import { EChart } from "./echart";

export interface Interactions {
  selected: Scalar[];
  /** A click on a category / slice / item: cross-filter. */
  onSelect?: (key: unknown, field: Field | undefined) => void;
  /** A double-click: drill into that value. */
  onDrill?: (key: unknown, field: Field | undefined) => void;
}

export function KpiVisual({ visual, result }: { visual: Visual; result: VisualResult }) {
  const format = visual.spec.options.format ?? undefined;
  const value = result.main ? toNumber(result.main.table.getChildAt(0)?.get(0)) : null;
  const previous = result.compare ? toNumber(result.compare.table.getChildAt(0)?.get(0)) : null;
  const delta = result.compare ? formatDelta(value, previous) : null;
  return (
    <div className="flex h-full flex-col justify-center gap-1.5">
      <span className="text-[28px] leading-none font-bold tracking-tight" data-testid="kpi-value">
        {formatValue(value, format)}
      </span>
      {delta ? (
        <span
          className={cn(
            "text-xs font-medium",
            delta.tone === "up" && "text-success",
            delta.tone === "down" && "text-destructive",
            delta.tone === "flat" && "text-muted-foreground",
          )}
        >
          {delta.text === "Flat" ? "Flat" : delta.text} vs previous period
        </span>
      ) : (
        <span className="text-xs text-muted-foreground">
          {visual.spec.measures[0]
            ? measureLabel(visual.spec.measures[0], result.main?.table.schema.fields[0]?.name)
            : ""}
        </span>
      )}
    </div>
  );
}

/** The design's region bars: a ranked list with values, for a handful of categories. */
export function BarList({
  visual,
  table,
  selected,
  onSelect,
  onDrill,
}: { visual: Visual; table: Table } & Interactions) {
  const { keys, labels, series } = useMemo(() => shape(visual.spec, table), [visual.spec, table]);
  const values = series[0]?.values ?? [];
  const max = Math.max(1e-9, ...values.map((v) => v ?? 0));
  const field = table.schema.fields[0];
  const format = visual.spec.options.format ?? undefined;
  return (
    <ul className="flex flex-col gap-3 overflow-auto" aria-label={visual.spec.title}>
      {labels.map((label, i) => {
        const key = keys[i];
        const isSelected = selected.includes(scalarOf(key, field) ?? "");
        const dimmed = selected.length > 0 && !isSelected;
        return (
          <li key={label}>
            <button
              type="button"
              aria-pressed={isSelected}
              onClick={() => onSelect?.(key, field)}
              onDoubleClick={() => onDrill?.(key, field)}
              className={cn(
                "group w-full rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring",
                dimmed && "opacity-40",
              )}
            >
              <span className="mb-1.5 flex items-baseline justify-between gap-3 text-sm">
                <span className="truncate font-medium">{label}</span>
                <span className="font-mono text-[13px] text-secondary-foreground tabular-nums">
                  {formatValue(values[i] ?? null, format)}
                </span>
              </span>
              <span className="block h-2.5 overflow-hidden rounded-full bg-secondary">
                <span
                  className="block h-full rounded-full bg-primary transition-[width] group-hover:bg-primary-hover"
                  style={{ width: `${Math.max(0, ((values[i] ?? 0) / max) * 100)}%` }}
                />
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

/** Line series past this many points are LTTB-downsampled in the compute worker. */
const DOWNSAMPLE_ABOVE = 1500;

/** Every ECharts-rendered kind. */
export const ChartVisual = memo(function ChartVisual({
  visual,
  table,
  selected,
  onSelect,
  onDrill,
}: { visual: Visual; table: Table } & Interactions) {
  const spec = visual.spec;
  const line = (spec.kind === "line" || spec.kind === "area") && !spec.series;
  const [kept, setKept] = useState<{ table: Table; indices: Uint32Array } | null>(null);
  const [zoom, setZoom] = useState<[number, number]>([0, 100]);

  // Long single series: keep only the points LTTB picks for the visible window, so pan and
  // zoom stay at 60 fps however long the series is.
  useEffect(() => {
    if (!line || table.numRows <= DOWNSAMPLE_ABOVE) return;
    const n = table.numRows;
    const ys = Float64Array.from(columnOf(table, 1), (v) => toNumber(v) ?? Number.NaN);
    const xs = Float64Array.from({ length: n }, (_, i) => i);
    const start = Math.floor((zoom[0] / 100) * n);
    const end = Math.ceil((zoom[1] / 100) * n);
    let cancelled = false;
    void compute()
      .call({ op: "lttb", xs, ys, start, end, threshold: DOWNSAMPLE_ABOVE })
      .then((indices) => {
        if (!cancelled) setKept({ table, indices });
      });
    return () => {
      cancelled = true;
    };
  }, [line, table, zoom]);

  const field = table.schema.fields[0];
  const shaped = useMemo(() => shape(spec, table), [spec, table]);
  const shown = kept?.table === table ? pickRows(shaped, kept.indices) : shaped;
  const option = useMemo<EChartsOption>(() => {
    switch (spec.kind) {
      case "pie":
      case "donut":
        return pieOption(spec, table, selected);
      case "heatmap":
        return heatmapOption(spec, table);
      case "histogram":
        return histogramOption(spec, table);
      default:
        return shapedOption(spec, shown, selected, field);
    }
  }, [spec, table, selected, shown, field]);

  const keys =
    spec.kind === "heatmap"
      ? columnOf(table, 0)
      : spec.kind === "pie" || spec.kind === "donut"
        ? foldSlices(shown).keys
        : shown.keys;
  const keyAt = (dataIndex: number, value: unknown): unknown => {
    if (spec.kind === "heatmap" && Array.isArray(value)) {
      const xLabels = [...new Set(keys.map((k) => formatDimension(k, field, spec.x)))];
      const label = xLabels[value[0] as number];
      return keys.find((k) => formatDimension(k, field, spec.x) === label);
    }
    return keys[dataIndex];
  };
  return (
    <EChart
      option={option}
      label={spec.title}
      onPick={onSelect ? (p) => onSelect(keyAt(p.dataIndex, p.value), field) : undefined}
      onDoublePick={onDrill ? (p) => onDrill(keyAt(p.dataIndex, p.value), field) : undefined}
      onZoom={line ? (start, end) => setZoom([start, end]) : undefined}
    />
  );
});

/** Rows × columns of a two-dimension result, summed into a matrix with totals. */
export function PivotVisual({ visual, table }: { visual: Visual; table: Table }) {
  const fields = table.schema.fields;
  const rowsCol = columnOf(table, 0);
  const colsCol = columnOf(table, 1);
  const values = columnOf(table, 2).map(toNumber);
  const rowLabels = [...new Set(rowsCol.map((v) => formatDimension(v, fields[0], visual.spec.x)))];
  const colLabels = [
    ...new Set(colsCol.map((v) => formatDimension(v, fields[1], visual.spec.series))),
  ];
  const cells = new Map<string, number>();
  rowsCol.forEach((r, i) => {
    const key = `${formatDimension(r, fields[0], visual.spec.x)}\u0000${formatDimension(colsCol[i], fields[1], visual.spec.series)}`;
    cells.set(key, (cells.get(key) ?? 0) + (values[i] ?? 0));
  });
  const format = visual.spec.options.format ?? undefined;
  const total = (pred: (k: string) => boolean) =>
    [...cells].filter(([k]) => pred(k)).reduce((s, [, v]) => s + v, 0);
  return (
    <div className="size-full overflow-auto">
      <table className="w-full border-collapse text-[13px]">
        <thead className="sticky top-0 bg-card">
          <tr className="border-b border-border text-secondary-foreground">
            <th className="px-2 py-1.5 text-left font-medium">{visual.spec.x?.column}</th>
            {colLabels.map((c) => (
              <th key={c} className="px-2 py-1.5 text-right font-medium">
                {c}
              </th>
            ))}
            <th className="px-2 py-1.5 text-right font-semibold">Total</th>
          </tr>
        </thead>
        <tbody>
          {rowLabels.map((r) => (
            <tr key={r} className="border-b border-border-subtle">
              <td className="px-2 py-1.5 font-medium whitespace-nowrap">{r}</td>
              {colLabels.map((c) => (
                <td key={c} className="px-2 py-1.5 text-right font-mono tabular-nums">
                  {formatValue(cells.get(`${r}\u0000${c}`) ?? null, format)}
                </td>
              ))}
              <td className="px-2 py-1.5 text-right font-mono font-semibold tabular-nums">
                {formatValue(
                  total((k) => k.startsWith(`${r}\u0000`)),
                  format,
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Pick values to filter the whole sheet (every visual with the column). */
export function SlicerVisual({
  visual,
  table,
  selected,
  onSelect,
}: { visual: Visual; table: Table } & Interactions) {
  const field = table.schema.fields[0];
  const keys = columnOf(table, 0);
  const counts = columnOf(table, 1).map(toNumber);
  return (
    <ul className="flex flex-col gap-0.5 overflow-auto" aria-label={visual.spec.title}>
      {keys.map((key, i) => {
        const scalar = scalarOf(key, field);
        const checked = scalar !== null && selected.includes(scalar);
        return (
          <li key={String(scalar)}>
            <button
              type="button"
              role="checkbox"
              aria-checked={checked}
              onClick={() => onSelect?.(key, field)}
              className="flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left text-sm hover:bg-secondary"
            >
              <span
                className={cn(
                  "grid size-4 flex-none place-items-center rounded border",
                  checked ? "border-primary bg-primary text-primary-foreground" : "border-input",
                )}
                aria-hidden
              >
                {checked ? "✓" : null}
              </span>
              <span className="flex-1 truncate">{formatDimension(key, field, visual.spec.x)}</span>
              <span className="font-mono text-xs text-muted-foreground tabular-nums">
                {formatValue(counts[i] ?? null, undefined)}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
