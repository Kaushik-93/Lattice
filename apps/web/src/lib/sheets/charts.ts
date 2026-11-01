import type { Measure, Scalar, VisualSpec } from "@lattice/shared-types";
import type { Field, Table } from "apache-arrow";
import type { EChartsOption } from "echarts";

import { columnOf, formatDimension, formatValue, numbersOf, scalarOf } from "@/lib/sheets/values";

/**
 * Categorical slots in fixed order (never cycled): the design's indigo and orange, then
 * hues from the validated reference palette. Validated as a set on the card surface
 * (scripts/validate_palette.js: CVD ΔE ≥ 9.1, normal-vision ΔE ≥ 19.6 for adjacent pairs).
 * Four slots sit under 3:1 contrast, so every chart keeps tooltips and a data-table view.
 */
export const PALETTE = [
  "#4338ca",
  "#ea7a2e",
  "#1baf7a",
  "#eda100",
  "#e87ba4",
  "#008300",
  "#2a78d6",
  "#e34948",
];
/** Series beyond the palette fold into "Other". Scatter plots validate only 3 slots. */
export const OTHER = "#9aa1ad";
export const SCATTER_SLOTS = 3;
/** One hue, light → dark, for magnitude (heatmaps). */
export const SEQUENTIAL = [
  "#eef0ff",
  "#c7cbfb",
  "#9ea3f5",
  "#7378ec",
  "#4f52dc",
  "#342aa5",
  "#261d7d",
];

const INK = { primary: "#101828", secondary: "#556070", grid: "#eef0f3", axis: "#e4e7ec" };
const MONO = "ui-monospace, SFMono-Regular, Menlo, monospace";

/** How a measure is named in legends and headers. A metric is named after its result column
 * (the gateway names it after the metric: "net_revenue" → "Net revenue"). */
export function measureLabel(m: Measure, field?: string): string {
  if (m.metric) {
    const words = (field ?? "metric").replace(/_+/g, " ").trim();
    return words.charAt(0).toUpperCase() + words.slice(1);
  }
  const of = m.column ? ` of ${m.column}` : "";
  switch (m.agg) {
    case "count":
      return m.column ? `Count of ${m.column}` : "Count";
    case "sum":
      return `Sum${of}`;
    case "avg":
      return `Average${of}`;
    case "min":
      return `Min${of}`;
    case "max":
      return `Max${of}`;
    case "count_distinct":
      return `Distinct ${m.column ?? ""}`.trim();
    case "approx_count_distinct":
      return `≈ distinct ${m.column ?? ""}`.trim();
  }
}

export interface Series {
  name: string;
  values: (number | null)[];
}

/** A grouped result reshaped for charting: one category per x value, one series per measure
 * (or per value of the split dimension). */
export interface Shaped {
  /** Raw x values, for turning a click back into a filter. */
  keys: unknown[];
  labels: string[];
  series: Series[];
}

export function shape(spec: VisualSpec, table: Table): Shaped {
  const fields = table.schema.fields;
  const xs = columnOf(table, 0);
  if (!spec.series) {
    // Measure columns follow the axis. A spec without measures (histograms, slicers) gets the
    // gateway's default count.
    return {
      keys: xs,
      labels: xs.map((v) => formatDimension(v, fields[0], spec.x)),
      series: fields.slice(1).map((field, i) => {
        const m = spec.measures[i];
        return {
          name: m ? measureLabel(m, field.name) : field.name === "count" ? "Count" : field.name,
          values: numbersOf(table, i + 1),
        };
      }),
    };
  }
  // x, split, measure → one series per split value, aligned on x.
  const splits = columnOf(table, 1);
  const values = numbersOf(table, 2);
  const keys: unknown[] = [];
  const keyIndex = new Map<unknown, number>();
  const series = new Map<string, (number | null)[]>();
  xs.forEach((x, row) => {
    let i = keyIndex.get(x);
    if (i === undefined) {
      i = keys.length;
      keyIndex.set(x, i);
      keys.push(x);
    }
    const name = formatDimension(splits[row], fields[1], spec.series);
    let s = series.get(name);
    if (!s) {
      s = [];
      series.set(name, s);
    }
    s[i] = values[row] ?? null;
  });
  return {
    keys,
    labels: keys.map((v) => formatDimension(v, fields[0], spec.x)),
    series: [...series].map(([name, vs]) => ({
      name,
      values: Array.from({ length: keys.length }, (_, i) => vs[i] ?? null),
    })),
  };
}

const baseTooltip = {
  backgroundColor: "#ffffff",
  borderColor: INK.axis,
  textStyle: { color: INK.primary, fontSize: 12 },
  extraCssText: "box-shadow: 0 4px 12px rgba(16,24,40,.08); border-radius: 8px;",
};

function legend(count: number): EChartsOption["legend"] {
  return count >= 2
    ? {
        top: 0,
        right: 0,
        icon: "roundRect",
        itemWidth: 12,
        itemHeight: 3,
        textStyle: { color: INK.secondary, fontSize: 12 },
      }
    : { show: false };
}

function colorOf(i: number): string {
  return PALETTE[i] ?? OTHER;
}

function keyMatches(key: unknown, selected: Scalar[], field: Field | undefined): boolean {
  const scalar = scalarOf(key, field);
  return selected.some((s) => s === scalar);
}

/** Only the categories at `indices` (a downsampled series). */
export function pickRows(shaped: Shaped, indices: ArrayLike<number>): Shaped {
  const at = <T>(xs: T[]) => Array.from(indices, (i) => xs[i] as T);
  return {
    keys: at(shaped.keys),
    labels: at(shaped.labels),
    series: shaped.series.map((s) => ({ name: s.name, values: at(s.values) })),
  };
}

/** Line, area, column and bar charts. */
export function cartesianOption(spec: VisualSpec, table: Table, selected: Scalar[]): EChartsOption {
  return shapedOption(spec, shape(spec, table), selected, table.schema.fields[0]);
}

export function shapedOption(
  spec: VisualSpec,
  shaped: Shaped,
  selected: Scalar[],
  field: Field | undefined,
): EChartsOption {
  const { keys, labels, series } = shaped;
  const format = spec.options.format ?? undefined;
  const horizontal = spec.kind === "bar";
  const line = spec.kind === "line" || spec.kind === "area";
  const stacked = spec.options.stacked && series.length > 1;
  const dim = selected.length > 0;
  const categoryAxis = {
    type: "category" as const,
    data: labels,
    axisLine: { lineStyle: { color: INK.axis } },
    axisTick: { show: false },
    axisLabel: { color: INK.secondary, fontSize: 11, fontFamily: MONO, hideOverlap: true },
    boundaryGap: !line,
  };
  const valueAxis = {
    type: "value" as const,
    splitLine: { lineStyle: { color: INK.grid } },
    axisLabel: {
      color: INK.secondary,
      fontSize: 11,
      fontFamily: MONO,
      formatter: (v: number) => formatValue(v, format),
    },
  };
  return {
    animationDuration: 250,
    animationDurationUpdate: 200,
    grid: { left: 4, right: 12, top: series.length >= 2 ? 28 : 12, bottom: 4, containLabel: true },
    legend: legend(series.length),
    tooltip: {
      ...baseTooltip,
      trigger: "axis",
      axisPointer: { type: line ? "line" : "shadow", lineStyle: { color: INK.axis } },
      valueFormatter: (v) =>
        formatValue(typeof v === "number" ? v : null, format, { compact: false }),
    },
    // Wheel/pinch zoom and brush-to-zoom along the category axis.
    dataZoom:
      line && keys.length > 24
        ? [{ type: "inside", [horizontal ? "yAxisIndex" : "xAxisIndex"]: 0 }]
        : [],
    xAxis: horizontal ? valueAxis : categoryAxis,
    yAxis: horizontal ? { ...categoryAxis, inverse: true } : valueAxis,
    series: series.map((s, i) => ({
      name: s.name,
      type: line ? "line" : "bar",
      stack: stacked ? "total" : undefined,
      data: s.values.map((v, j) => ({
        value: v,
        itemStyle:
          dim && !line && !keyMatches(keys[j], selected, field) ? { opacity: 0.3 } : undefined,
      })),
      color: colorOf(i),
      showSymbol: false,
      symbolSize: 8,
      lineStyle: line ? { width: 2 } : undefined,
      areaStyle: spec.kind === "area" ? { opacity: series.length > 1 ? 0.12 : 0.1 } : undefined,
      // Rounded data ends, 2px surface gaps between neighbouring bars.
      itemStyle: line
        ? undefined
        : {
            borderRadius: stacked ? 0 : horizontal ? [0, 4, 4, 0] : [4, 4, 0, 0],
            borderColor: "#ffffff",
            borderWidth: 1,
          },
      barMaxWidth: 48,
      emphasis: { focus: "series" },
    })),
  };
}

/** Slices before the rest fold into "Other" (one per categorical slot, never a 9th hue). */
export const PIE_SLICES = PALETTE.length;

/** The largest slices, plus "Other" (key undefined) summing the rest. */
export function foldSlices(shaped: Shaped): Shaped {
  if (shaped.keys.length <= PIE_SLICES) return shaped;
  const keep = PIE_SLICES - 1;
  const values = shaped.series[0]?.values ?? [];
  const rest = values.slice(keep).reduce<number>((sum, v) => sum + (v ?? 0), 0);
  return {
    keys: [...shaped.keys.slice(0, keep), undefined],
    labels: [...shaped.labels.slice(0, keep), "Other"],
    series: shaped.series
      .slice(0, 1)
      .map((s) => ({ name: s.name, values: [...values.slice(0, keep), rest] })),
  };
}

export function pieOption(spec: VisualSpec, table: Table, selected: Scalar[]): EChartsOption {
  const { keys, labels, series } = foldSlices(shape(spec, table));
  const values = series[0]?.values ?? [];
  const format = spec.options.format ?? undefined;
  const dim = selected.length > 0;
  return {
    animationDuration: 250,
    legend: { ...legend(2), orient: "vertical", top: "middle", right: 0 },
    tooltip: {
      ...baseTooltip,
      trigger: "item",
      valueFormatter: (v) =>
        formatValue(typeof v === "number" ? v : null, format, { compact: false }),
    },
    series: [
      {
        type: "pie",
        radius: spec.kind === "donut" ? ["52%", "78%"] : "78%",
        center: ["38%", "50%"],
        padAngle: 1,
        itemStyle: { borderRadius: 4, borderColor: "#ffffff", borderWidth: 2 },
        label: { show: false },
        data: labels.map((name, i) => ({
          name,
          value: values[i] ?? 0,
          itemStyle: {
            color: keys[i] === undefined ? OTHER : colorOf(i),
            opacity: dim && !keyMatches(keys[i], selected, table.schema.fields[0]) ? 0.3 : 1,
          },
        })),
      },
    ],
  };
}

export function heatmapOption(spec: VisualSpec, table: Table): EChartsOption {
  const fields = table.schema.fields;
  const xs = columnOf(table, 0);
  const ys = columnOf(table, 1);
  const values = numbersOf(table, 2);
  const xLabels = [...new Set(xs.map((v) => formatDimension(v, fields[0], spec.x)))];
  const yLabels = [...new Set(ys.map((v) => formatDimension(v, fields[1], spec.series)))];
  const finite = values.filter((v): v is number => v !== null);
  const format = spec.options.format ?? undefined;
  return {
    animation: false,
    grid: { left: 4, right: 12, top: 8, bottom: 40, containLabel: true },
    tooltip: {
      ...baseTooltip,
      trigger: "item",
      valueFormatter: (v) => {
        const n = Array.isArray(v) ? v[2] : v;
        return formatValue(typeof n === "number" ? n : null, format, { compact: false });
      },
    },
    xAxis: {
      type: "category",
      data: xLabels,
      axisTick: { show: false },
      axisLine: { show: false },
      axisLabel: { color: INK.secondary, fontSize: 11 },
    },
    yAxis: {
      type: "category",
      data: yLabels,
      axisTick: { show: false },
      axisLine: { show: false },
      axisLabel: { color: INK.secondary, fontSize: 11 },
    },
    visualMap: {
      min: finite.length ? Math.min(...finite) : 0,
      max: finite.length ? Math.max(...finite) : 1,
      calculable: false,
      orient: "horizontal",
      left: "center",
      bottom: 0,
      itemHeight: 120,
      itemWidth: 10,
      inRange: { color: SEQUENTIAL },
      text: [
        formatValue(finite.length ? Math.max(...finite) : 1, format),
        formatValue(finite.length ? Math.min(...finite) : 0, format),
      ],
      textStyle: { color: INK.secondary, fontSize: 11 },
    },
    series: [
      {
        type: "heatmap",
        data: xs.map((x, i) => [
          xLabels.indexOf(formatDimension(x, fields[0], spec.x)),
          yLabels.indexOf(formatDimension(ys[i], fields[1], spec.series)),
          values[i],
        ]),
        itemStyle: { borderColor: "#ffffff", borderWidth: 2, borderRadius: 4 },
        emphasis: { itemStyle: { borderColor: INK.primary, borderWidth: 1 } },
      },
    ],
  };
}

/** Histograms: one column per bin, labelled with its range. */
export function histogramOption(spec: VisualSpec, table: Table): EChartsOption {
  const option = cartesianOption({ ...spec, kind: "column" }, table, []);
  return {
    ...option,
    series: (option.series as object[]).map((s) => ({ ...s, barCategoryGap: "4%" })),
  };
}
