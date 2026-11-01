"use client";

import type {
  Aggregation,
  Dimension,
  Measure,
  ModelMetric,
  ProjectModelResponse,
  SheetFilterDef,
  TimeBucket,
  ValueFormat,
  Visual,
  VisualKind,
  VisualSpec,
} from "@lattice/shared-types";
import { Plus, Trash2, X } from "lucide-react";
import { useId, useState, type ReactNode } from "react";

import { dataClient } from "@/lib/data/client";
import { metricsFor, useProjectModel } from "@/lib/knowledge/api";
import { useProjectTables, type CatalogTable } from "@/lib/queries";
import { newId, type SheetDoc } from "@/lib/sheets/store";
import { cn } from "@/lib/utils";

import { useSheetEnv, useSheetState } from "./sheet-env";

export const KINDS: [VisualKind, string][] = [
  ["kpi", "KPI"],
  ["column", "Column"],
  ["bar", "Bar"],
  ["line", "Line"],
  ["area", "Area"],
  ["pie", "Pie"],
  ["donut", "Donut"],
  ["scatter", "Scatter"],
  ["histogram", "Histogram"],
  ["heatmap", "Heatmap"],
  ["table", "Table"],
  ["pivot", "Pivot"],
  ["slicer", "Slicer"],
];

const AGGS: [Aggregation, string][] = [
  ["sum", "Sum"],
  ["avg", "Average"],
  ["count", "Count"],
  ["count_distinct", "Distinct count"],
  ["approx_count_distinct", "≈ Distinct count"],
  ["min", "Min"],
  ["max", "Max"],
];
const BUCKETS: TimeBucket[] = ["day", "week", "month", "quarter", "year"];
const SPLIT_KINDS = new Set<VisualKind>([
  "line",
  "area",
  "bar",
  "column",
  "scatter",
  "pivot",
  "heatmap",
  "table",
]);
const SINGLE_MEASURE = new Set<VisualKind>(["kpi", "pie", "donut", "heatmap"]);

const NUMERIC =
  /^(TINYINT|SMALLINT|INTEGER|INT|BIGINT|HUGEINT|UTINYINT|USMALLINT|UINTEGER|UBIGINT|FLOAT|REAL|DOUBLE|DECIMAL|NUMERIC)/i;
const TEMPORAL = /^(DATE|TIMESTAMP)/i;
export const isNumericType = (t: string) => NUMERIC.test(t);
export const isTemporalType = (t: string) => TEMPORAL.test(t);

/** A sensible starting visual on `table`: a count by its first text column. */
export function defaultSpec(table: CatalogTable): VisualSpec {
  const category = table.columns.find(
    (c) => !isNumericType(c.data_type) && !isTemporalType(c.data_type),
  );
  const amount = table.columns.find((c) => isNumericType(c.data_type) && !/id$/i.test(c.name));
  return {
    kind: "column",
    title: category ? `By ${category.name}` : table.name,
    table_id: table.id,
    x: category ? { column: category.name, bucket: null } : undefined,
    measures: [amount ? { column: amount.name, agg: "sum" } : { column: null, agg: "count" }],
    columns: [],
    drill: [],
    filters: [],
    options: { stacked: false },
  };
}

/** Round bin width giving about 20 bins over the profiled range. */
export function binWidth(min: string | null, max: string | null): number {
  const lo = Number(min);
  const hi = Number(max);
  if (!Number.isFinite(lo) || !Number.isFinite(hi) || hi <= lo) return 1;
  const raw = (hi - lo) / 20;
  const magnitude = 10 ** Math.floor(Math.log10(raw));
  return ([1, 2, 2.5, 5, 10].find((m) => m * magnitude >= raw) ?? 10) * magnitude;
}

/** Keeps a spec valid for its (new) kind. */
export function adaptSpec(
  spec: VisualSpec,
  kind: VisualKind,
  table: CatalogTable | undefined,
): VisualSpec {
  const next: VisualSpec = { ...spec, kind };
  const numeric = table?.columns.filter((c) => isNumericType(c.data_type)) ?? [];
  if (SINGLE_MEASURE.has(kind)) next.measures = spec.measures.slice(0, 1);
  if (kind !== "kpi" && kind !== "table" && !next.x && table) next.x = defaultSpec(table).x;
  if (kind === "histogram") {
    const col =
      numeric.find((c) => c.name === spec.x?.column) ??
      numeric.find((c) => !/id$/i.test(c.name)) ??
      numeric[0];
    if (col) next.x = { column: col.name, bucket: null, bin: binWidth(col.min, col.max) };
    next.measures = [];
  } else if (next.x?.bin !== undefined) {
    next.x = { column: next.x.column, bucket: next.x.bucket };
  }
  if (kind === "scatter") {
    const y = numeric.find((c) => !/id$/i.test(c.name)) ?? numeric[0];
    next.columns = y ? [y.name] : [];
    const temporal = table?.columns.find((c) => isTemporalType(c.data_type));
    if (temporal) next.x = { column: temporal.name, bucket: null };
    next.limit = 1_000_000;
  } else if (spec.kind === "scatter") {
    next.columns = [];
    next.limit = undefined;
  }
  if ((kind === "heatmap" || kind === "pivot") && !next.series && table) {
    const other = table.columns.find(
      (c) =>
        !isNumericType(c.data_type) && !isTemporalType(c.data_type) && c.name !== next.x?.column,
    );
    if (other) next.series = { column: other.name, bucket: null };
  }
  if (!SPLIT_KINDS.has(kind)) next.series = undefined;
  if (
    next.measures.length === 0 &&
    kind !== "histogram" &&
    kind !== "slicer" &&
    kind !== "scatter" &&
    kind !== "table"
  ) {
    next.measures = [{ column: null, agg: "count" }];
  }
  if (kind === "kpi") next.x = undefined;
  return next;
}

function Field({ label, children }: { label: string; children: (id: string) => ReactNode }) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="text-xs font-semibold text-secondary-foreground">
        {label}
      </label>
      {children(id)}
    </div>
  );
}

const control =
  "h-9 w-full rounded-md border border-input bg-card px-2.5 text-sm focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

/** The right-hand panel while editing: the selected visual's fields, or the sheet's settings. */
export function EditorPanel() {
  const { store, target } = useSheetEnv();
  const projectId = target.kind === "member" ? target.projectId : "";
  const { tables } = useProjectTables(projectId);
  const model = useProjectModel(projectId || null).data;
  const selected = useSheetState((s) => s.doc.visuals.find((v) => v.id === s.selectedVisual));
  const close = () => store.getState().setEditing(false);
  return (
    <aside
      aria-label="Editor"
      className="flex w-full flex-col border-t border-border bg-card max-lg:mt-4 max-lg:rounded-xl max-lg:border lg:fixed lg:top-0 lg:right-0 lg:z-20 lg:h-screen lg:w-[340px] lg:border-t-0 lg:border-l lg:shadow-[-8px_0_24px_rgba(16,24,40,0.04)]"
    >
      <div className="flex items-center gap-2 border-b border-border px-5 py-4">
        <h2 className="flex-1 text-base font-semibold">{selected ? "Edit visual" : "Sheet"}</h2>
        {selected && (
          <button
            type="button"
            className="text-sm text-primary hover:underline"
            onClick={() => store.getState().selectVisual(null)}
          >
            Sheet settings
          </button>
        )}
        <button
          type="button"
          aria-label="Done editing"
          onClick={close}
          className="grid size-8 place-items-center rounded-md hover:bg-secondary"
        >
          <X className="size-4" />
        </button>
      </div>
      <div className="flex-1 overflow-y-auto px-5 py-4">
        {selected ? (
          <VisualFields key={selected.id} visual={selected} tables={tables} model={model} />
        ) : (
          <SheetFields tables={tables} projectId={projectId} />
        )}
      </div>
    </aside>
  );
}

function VisualFields({
  visual,
  tables,
  model,
}: {
  visual: Visual;
  tables: CatalogTable[];
  model: ProjectModelResponse | undefined;
}) {
  const { store } = useSheetEnv();
  const spec = visual.spec;
  const table = tables.find((t) => t.id === spec.table_id);
  const columns = table?.columns ?? [];
  const update = (label: string, patch: (s: VisualSpec) => VisualSpec, merge?: string) =>
    store.getState().edit(
      label,
      (doc) => ({
        ...doc,
        visuals: doc.visuals.map((v) => (v.id === visual.id ? { ...v, spec: patch(v.spec) } : v)),
      }),
      merge,
    );
  const columnOptions = (pred: (t: string) => boolean = () => true) =>
    columns
      .filter((c) => pred(c.data_type))
      .map((c) => (
        <option key={c.name} value={c.name}>
          {c.name}
        </option>
      ));
  const typeOf = (name: string | undefined) =>
    columns.find((c) => c.name === name)?.data_type ?? "";
  const dimField = (
    label: string,
    dim: Dimension | undefined,
    set: (d: Dimension | undefined) => void,
    optional: boolean,
  ) => (
    <Field label={label}>
      {(id) => (
        <div className="flex gap-2">
          <select
            id={id}
            className={control}
            value={dim?.column ?? ""}
            onChange={(e) =>
              set(
                e.target.value
                  ? {
                      column: e.target.value,
                      bucket: isTemporalType(typeOf(e.target.value)) ? "month" : null,
                    }
                  : undefined,
              )
            }
          >
            {optional && <option value="">None</option>}
            {columnOptions()}
          </select>
          {dim && isTemporalType(typeOf(dim.column)) && (
            <select
              aria-label={`${label} grain`}
              className={cn(control, "w-28")}
              value={dim.bucket ?? ""}
              onChange={(e) =>
                set({ ...dim, bucket: (e.target.value || null) as TimeBucket | null })
              }
            >
              <option value="">Exact</option>
              {BUCKETS.map((b) => (
                <option key={b} value={b}>
                  {b[0]!.toUpperCase() + b.slice(1)}
                </option>
              ))}
            </select>
          )}
        </div>
      )}
    </Field>
  );

  const usesX = spec.kind !== "kpi" && !(spec.kind === "table" && !spec.x);
  return (
    <div className="grid gap-4">
      <Field label="Title">
        {(id) => (
          <input
            id={id}
            className={control}
            value={spec.title}
            maxLength={120}
            onChange={(e) =>
              update(
                "Rename visual",
                (s) => ({ ...s, title: e.target.value }),
                `title:${visual.id}`,
              )
            }
          />
        )}
      </Field>
      <fieldset className="grid gap-1.5">
        <legend className="mb-1.5 text-xs font-semibold text-secondary-foreground">Type</legend>
        <div className="grid grid-cols-3 gap-1.5">
          {KINDS.map(([kind, label]) => (
            <button
              key={kind}
              type="button"
              aria-pressed={spec.kind === kind}
              onClick={() => update("Change visual type", (s) => adaptSpec(s, kind, table))}
              className={cn(
                "h-8 rounded-md border text-xs font-medium",
                spec.kind === kind
                  ? "border-primary bg-accent text-accent-foreground"
                  : "border-border hover:border-input",
              )}
            >
              {label}
            </button>
          ))}
        </div>
      </fieldset>
      <Field label="Table">
        {(id) => (
          <select
            id={id}
            className={control}
            value={spec.table_id}
            onChange={(e) => {
              const t = tables.find((x) => x.id === e.target.value);
              if (t)
                update("Change table", () =>
                  adaptSpec({ ...defaultSpec(t), title: spec.title }, spec.kind, t),
                );
            }}
          >
            {tables.map((t) => (
              <option key={t.id} value={t.id}>
                {t.source} · {t.name}
              </option>
            ))}
          </select>
        )}
      </Field>

      {spec.kind === "table" && (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={!!spec.x || spec.measures.length > 0}
            onChange={(e) =>
              update("Change table mode", (s) =>
                e.target.checked
                  ? {
                      ...s,
                      x: defaultSpec(table ?? tables[0]!).x,
                      measures: [{ column: null, agg: "count" }],
                      columns: [],
                    }
                  : {
                      ...s,
                      x: undefined,
                      series: undefined,
                      measures: [],
                      columns: columns.slice(0, 8).map((c) => c.name),
                    },
              )
            }
          />
          Summarise (group and aggregate)
        </label>
      )}

      {spec.kind === "histogram" ? (
        <>
          <Field label="Values">
            {(id) => (
              <select
                id={id}
                className={control}
                value={spec.x?.column ?? ""}
                onChange={(e) => {
                  const c = columns.find((x) => x.name === e.target.value);
                  if (c)
                    update("Change histogram column", (s) => ({
                      ...s,
                      x: { column: c.name, bucket: null, bin: binWidth(c.min, c.max) },
                    }));
                }}
              >
                {columnOptions(isNumericType)}
              </select>
            )}
          </Field>
          <Field label="Bin width">
            {(id) => (
              <input
                id={id}
                type="number"
                min="0"
                step="any"
                className={control}
                value={spec.x?.bin ?? 1}
                onChange={(e) => {
                  const w = Number(e.target.value);
                  if (w > 0 && spec.x)
                    update(
                      "Change bin width",
                      (s) => ({ ...s, x: { ...s.x!, bin: w } }),
                      `bin:${visual.id}`,
                    );
                }}
              />
            )}
          </Field>
        </>
      ) : (
        usesX &&
        dimField(
          spec.kind === "scatter"
            ? "X axis"
            : spec.kind === "pivot"
              ? "Rows"
              : spec.kind === "slicer"
                ? "Values of"
                : spec.kind === "pie" || spec.kind === "donut"
                  ? "Slices"
                  : "Group by",
          spec.x,
          (d) => update("Change axis", (s) => ({ ...s, x: d, drill: [] })),
          false,
        )
      )}

      {spec.kind === "scatter" && (
        <Field label="Y axis">
          {(id) => (
            <select
              id={id}
              className={control}
              value={spec.columns[0] ?? ""}
              onChange={(e) =>
                update("Change y axis", (s) => ({ ...s, columns: [e.target.value] }))
              }
            >
              {columnOptions(isNumericType)}
            </select>
          )}
        </Field>
      )}

      {SPLIT_KINDS.has(spec.kind) &&
        usesX &&
        dimField(
          spec.kind === "scatter"
            ? "Colour by"
            : spec.kind === "pivot"
              ? "Columns"
              : spec.kind === "heatmap"
                ? "Y axis"
                : "Split by",
          spec.series,
          (d) => update("Change split", (s) => ({ ...s, series: d })),
          spec.kind !== "heatmap" && spec.kind !== "pivot",
        )}

      {spec.kind !== "scatter" &&
        spec.kind !== "slicer" &&
        spec.kind !== "histogram" &&
        !(spec.kind === "table" && !spec.x) && (
          <MeasuresField
            measures={spec.measures}
            single={SINGLE_MEASURE.has(spec.kind)}
            columns={columns}
            metrics={metricsFor(model, spec.table_id)}
            onChange={(measures) => update("Change measures", (s) => ({ ...s, measures }))}
          />
        )}

      {spec.kind === "table" && !spec.x && spec.measures.length === 0 && (
        <fieldset className="grid gap-1">
          <legend className="mb-1 text-xs font-semibold text-secondary-foreground">Columns</legend>
          {columns.map((c) => (
            <label key={c.name} className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={spec.columns.length === 0 || spec.columns.includes(c.name)}
                onChange={(e) =>
                  update("Change columns", (s) => {
                    const current = s.columns.length ? s.columns : columns.map((x) => x.name);
                    const next = e.target.checked
                      ? [...current, c.name]
                      : current.filter((x) => x !== c.name);
                    return {
                      ...s,
                      columns: columns.map((x) => x.name).filter((x) => next.includes(x)),
                    };
                  })
                }
              />
              {c.name}
            </label>
          ))}
        </fieldset>
      )}

      {spec.kind === "kpi" && (
        <Field label="Compare with the previous period of">
          {(id) => (
            <select
              id={id}
              className={control}
              value={spec.options.compare ?? ""}
              onChange={(e) =>
                update("Change comparison", (s) => ({
                  ...s,
                  options: { ...s.options, compare: e.target.value || undefined },
                }))
              }
            >
              <option value="">No comparison</option>
              {columnOptions(isTemporalType)}
            </select>
          )}
        </Field>
      )}

      {["bar", "column", "area"].includes(spec.kind) && spec.series && (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={spec.options.stacked}
            onChange={(e) =>
              update("Toggle stacking", (s) => ({
                ...s,
                options: { ...s.options, stacked: e.target.checked },
              }))
            }
          />
          Stack series
        </label>
      )}

      {spec.kind !== "kpi" && spec.kind !== "scatter" && spec.kind !== "slicer" && (
        <div className="grid grid-cols-2 gap-2">
          <Field label="Sort">
            {(id) => (
              <select
                id={id}
                className={control}
                value={spec.sort ? `${spec.sort.by}:${spec.sort.dir}` : ""}
                onChange={(e) => {
                  const [by, dir] = e.target.value.split(":");
                  update("Change sort", (s) => ({
                    ...s,
                    sort: by ? { by: by as "x" | "value", dir: dir as "asc" | "desc" } : undefined,
                  }));
                }}
              >
                <option value="">Default</option>
                <option value="value:desc">Largest first</option>
                <option value="value:asc">Smallest first</option>
                <option value="x:asc">A → Z / oldest</option>
                <option value="x:desc">Z → A / newest</option>
              </select>
            )}
          </Field>
          <Field label="Max rows">
            {(id) => (
              <input
                id={id}
                type="number"
                min="1"
                className={control}
                placeholder="Default"
                value={spec.limit ?? ""}
                onChange={(e) =>
                  update(
                    "Change row limit",
                    (s) => ({
                      ...s,
                      limit: e.target.value ? Math.max(1, Number(e.target.value)) : undefined,
                    }),
                    `limit:${visual.id}`,
                  )
                }
              />
            )}
          </Field>
        </div>
      )}

      {spec.kind !== "scatter" && spec.kind !== "slicer" && (
        <Field label="Number format">
          {(id) => (
            <select
              id={id}
              className={control}
              value={spec.options.format ?? ""}
              onChange={(e) =>
                update("Change format", (s) => ({
                  ...s,
                  options: {
                    ...s.options,
                    format: (e.target.value || undefined) as ValueFormat | undefined,
                  },
                }))
              }
            >
              <option value="">Automatic</option>
              <option value="number">Number</option>
              <option value="currency">Currency</option>
              <option value="percent">Percent</option>
            </select>
          )}
        </Field>
      )}

      {["bar", "column", "line", "area", "pie", "donut"].includes(spec.kind) &&
        spec.x &&
        !spec.x.bucket && (
          <DrillField
            drill={spec.drill}
            columns={columns.map((c) => c.name).filter((c) => c !== spec.x?.column)}
            onChange={(drill) => update("Change drill path", (s) => ({ ...s, drill }))}
          />
        )}
    </div>
  );
}

const RAW = "raw";

/** A metric as a measure (`agg` is ignored for metrics). */
const metricMeasure = (id: string): Measure => ({ column: null, agg: "sum", metric: id });

function rawMeasure(columns: CatalogTable["columns"]): Measure {
  const numeric = columns.find((c) => isNumericType(c.data_type));
  return numeric ? { column: numeric.name, agg: "sum" } : { column: null, agg: "count" };
}

/** Metrics first: approved business definitions, then raw column aggregations (not governed).
 * A visual uses one or the other, since a raw aggregation has no grain to join on. */
function MeasuresField({
  measures,
  single,
  columns,
  metrics,
  onChange,
}: {
  measures: Measure[];
  single: boolean;
  columns: CatalogTable["columns"];
  metrics: ModelMetric[];
  onChange: (m: Measure[]) => void;
}) {
  const set = (i: number, m: Measure) => onChange(measures.map((x, j) => (j === i ? m : x)));
  const governed = measures.some((m) => m.metric);
  const unused = metrics.find((x) => !measures.some((m) => m.metric === x.note_id));
  const others = (i: number) => measures.filter((_, j) => j !== i);
  return (
    <fieldset className="grid gap-2">
      <legend className="mb-1 text-xs font-semibold text-secondary-foreground">
        {single ? "Measure" : "Measures"}
      </legend>
      {measures.map((m, i) => (
        <div key={i} className="grid gap-1.5">
          {(metrics.length > 0 || m.metric) && (
            <select
              aria-label="Metric"
              className={control}
              value={m.metric ?? RAW}
              onChange={(e) =>
                set(i, e.target.value === RAW ? rawMeasure(columns) : metricMeasure(e.target.value))
              }
            >
              <optgroup label="Metrics">
                {metrics.map((metric) => (
                  <option
                    key={metric.note_id}
                    value={metric.note_id}
                    disabled={others(i).some((o) => !o.metric)}
                  >
                    {metric.title}
                    {metric.standing === "unapproved" ? " · not approved" : ""}
                  </option>
                ))}
                {m.metric && !metrics.some((x) => x.note_id === m.metric) && (
                  <option value={m.metric}>Unavailable metric</option>
                )}
              </optgroup>
              <option value={RAW} disabled={others(i).some((o) => o.metric)}>
                Raw column (not governed)
              </option>
            </select>
          )}
          {!m.metric && <RawMeasure m={m} columns={columns} onChange={(next) => set(i, next)} />}
          {measures.length > 1 && (
            <button
              type="button"
              onClick={() => onChange(measures.filter((_, j) => j !== i))}
              className="flex items-center gap-1 justify-self-end text-xs text-subtle-foreground hover:text-foreground"
            >
              <Trash2 className="size-3.5" /> Remove
            </button>
          )}
        </div>
      ))}
      {!single && measures.length < 6 && !(governed && !unused) && (
        <button
          type="button"
          onClick={() =>
            onChange([
              ...measures,
              governed && unused ? metricMeasure(unused.note_id) : { column: null, agg: "count" },
            ])
          }
          className="flex items-center gap-1.5 text-sm font-medium text-primary hover:underline"
        >
          <Plus className="size-4" /> Add measure
        </button>
      )}
    </fieldset>
  );
}

function RawMeasure({
  m,
  columns,
  onChange,
}: {
  m: Measure;
  columns: CatalogTable["columns"];
  onChange: (m: Measure) => void;
}) {
  return (
    <div className="flex gap-1.5">
      <select
        aria-label="Aggregation"
        className={cn(control, "w-32 flex-none")}
        value={m.agg}
        onChange={(e) => {
          const agg = e.target.value as Aggregation;
          const numeric = agg === "sum" || agg === "avg";
          const column =
            agg === "count"
              ? null
              : m.column &&
                  (!numeric ||
                    isNumericType(columns.find((c) => c.name === m.column)?.data_type ?? ""))
                ? m.column
                : (columns.find((c) => (numeric ? isNumericType(c.data_type) : true))?.name ??
                  null);
          onChange({ agg, column });
        }}
      >
        {AGGS.map(([agg, label]) => (
          <option key={agg} value={agg}>
            {label}
          </option>
        ))}
      </select>
      <select
        aria-label="Of column"
        className={control}
        value={m.column ?? ""}
        onChange={(e) => onChange({ ...m, column: e.target.value || null })}
      >
        {m.agg === "count" && <option value="">rows</option>}
        {columns
          .filter((c) => (m.agg === "sum" || m.agg === "avg" ? isNumericType(c.data_type) : true))
          .map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
            </option>
          ))}
      </select>
    </div>
  );
}

function DrillField({
  drill,
  columns,
  onChange,
}: {
  drill: Dimension[];
  columns: string[];
  onChange: (d: Dimension[]) => void;
}) {
  const [pick, setPick] = useState("");
  return (
    <fieldset className="grid gap-1.5">
      <legend className="mb-1 text-xs font-semibold text-secondary-foreground">
        Drill down into
      </legend>
      {drill.map((d, i) => (
        <div key={d.column} className="flex items-center gap-2 text-sm">
          <span className="flex-1">
            {i + 1}. {d.column}
          </span>
          <button
            type="button"
            aria-label={`Remove ${d.column}`}
            onClick={() => onChange(drill.filter((_, j) => j !== i))}
            className="grid size-7 place-items-center rounded-md hover:bg-secondary"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
      <div className="flex gap-1.5">
        <select
          aria-label="Add drill level"
          className={control}
          value={pick}
          onChange={(e) => setPick(e.target.value)}
        >
          <option value="">Add a level…</option>
          {columns
            .filter((c) => !drill.some((d) => d.column === c))
            .map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
        </select>
        <button
          type="button"
          disabled={!pick}
          onClick={() => {
            onChange([...drill, { column: pick, bucket: null }]);
            setPick("");
          }}
          className="h-9 rounded-md border border-input px-3 text-sm font-medium disabled:opacity-50"
        >
          Add
        </button>
      </div>
    </fieldset>
  );
}

/** Distinct values of `column` (for a values filter's picker), at most 200. */
async function distinctValues(
  projectId: string,
  tableId: string,
  column: string,
): Promise<(string | number | boolean)[]> {
  const outcome = await dataClient("options").run(projectId, [
    {
      id: "options",
      query: {
        table_id: tableId,
        dimensions: [{ column, bucket: null }],
        measures: [{ column: null, agg: "count" }],
        filters: [],
        sort: [],
        limit: 200,
        offset: null,
      },
    },
  ]);
  const table = outcome.results.get("options")?.table;
  if (!table) return [];
  return Array.from(table.getChildAt(0) ?? [], (v) =>
    typeof v === "bigint"
      ? Number(v)
      : typeof v === "number" || typeof v === "boolean"
        ? v
        : String(v ?? ""),
  ).filter((v) => v !== "");
}

function SheetFields({ tables, projectId }: { tables: CatalogTable[]; projectId: string }) {
  const { store } = useSheetEnv();
  const doc = useSheetState((s) => s.doc);
  const [column, setColumn] = useState("");
  const [busy, setBusy] = useState(false);
  const used = new Set(doc.visuals.map((v) => v.spec.table_id));
  const candidates = tables.filter((t) => used.size === 0 || used.has(t.id));
  const allColumns = [
    ...new Map(
      candidates.flatMap((t) => t.columns.map((c) => [c.name, { ...c, table: t.id }] as const)),
    ).values(),
  ];
  const edit = (label: string, recipe: (d: SheetDoc) => SheetDoc, merge?: string) =>
    store.getState().edit(label, recipe, merge);

  const addFilter = async () => {
    const c = allColumns.find((x) => x.name === column);
    if (!c) return;
    setBusy(true);
    const temporal = isTemporalType(c.data_type);
    const options = temporal
      ? []
      : await distinctValues(projectId, c.table, c.name).catch(() => []);
    setBusy(false);
    const def: SheetFilterDef = {
      id: newId(),
      column: c.name,
      label: c.name.replace(/_/g, " ").replace(/^\w/, (ch) => ch.toUpperCase()),
      kind: temporal ? "date_range" : "values",
      options,
    };
    edit("Add filter", (d) => ({ ...d, filters: [...d.filters, def] }));
    setColumn("");
  };

  return (
    <div className="grid gap-5">
      <Field label="Sheet name">
        {(id) => (
          <input
            id={id}
            className={control}
            maxLength={80}
            value={doc.name}
            onChange={(e) =>
              edit("Rename sheet", (d) => ({ ...d, name: e.target.value }), "sheet-name")
            }
          />
        )}
      </Field>
      <section className="grid gap-2">
        <h3 className="text-xs font-semibold text-secondary-foreground">Filters</h3>
        {doc.filters.length === 0 && (
          <p className="text-sm text-muted-foreground">No filters yet.</p>
        )}
        {doc.filters.map((f) => (
          <div
            key={f.id}
            className="flex items-center gap-2 rounded-md border border-border px-2.5 py-2"
          >
            <input
              aria-label={`Label of the ${f.column} filter`}
              className="min-w-0 flex-1 bg-transparent text-sm outline-none"
              value={f.label}
              maxLength={60}
              onChange={(e) =>
                edit(
                  "Rename filter",
                  (d) => ({
                    ...d,
                    filters: d.filters.map((x) =>
                      x.id === f.id ? { ...x, label: e.target.value } : x,
                    ),
                  }),
                  `filter:${f.id}`,
                )
              }
            />
            <span className="text-xs text-muted-foreground">
              {f.kind === "date_range" ? "date range" : `${f.options?.length ?? 0} values`}
            </span>
            <button
              type="button"
              aria-label={`Remove the ${f.column} filter`}
              onClick={() =>
                edit("Remove filter", (d) => ({
                  ...d,
                  filters: d.filters.filter((x) => x.id !== f.id),
                }))
              }
              className="grid size-7 place-items-center rounded-md hover:bg-secondary"
            >
              <Trash2 className="size-3.5 text-subtle-foreground" />
            </button>
          </div>
        ))}
        <div className="flex gap-1.5">
          <select
            aria-label="Filter on column"
            className={control}
            value={column}
            onChange={(e) => setColumn(e.target.value)}
          >
            <option value="">Add a filter on…</option>
            {allColumns
              .filter((c) => !doc.filters.some((f) => f.column === c.name))
              .map((c) => (
                <option key={c.name} value={c.name}>
                  {c.name}
                </option>
              ))}
          </select>
          <button
            type="button"
            disabled={!column || busy}
            onClick={() => void addFilter()}
            className="h-9 rounded-md border border-input px-3 text-sm font-medium disabled:opacity-50"
          >
            Add
          </button>
        </div>
        <p className="text-xs text-muted-foreground">
          A filter applies to every visual whose table has the column.
        </p>
      </section>
    </div>
  );
}

/** Appends a starting visual (on the table the sheet uses most) and selects it for editing. */
export function addVisual(
  store: ReturnType<typeof useSheetEnv>["store"],
  tables: CatalogTable[],
): void {
  const uses = new Map<string, number>();
  for (const v of store.getState().doc.visuals) {
    uses.set(v.spec.table_id, (uses.get(v.spec.table_id) ?? 0) + 1);
  }
  const favourite = [...uses].sort((a, b) => b[1] - a[1])[0]?.[0];
  // On an empty sheet, the largest table: usually the facts people chart.
  const largest = [...tables].sort((a, b) => (b.rowCount ?? 0) - (a.rowCount ?? 0))[0];
  const table = tables.find((t) => t.id === favourite) ?? largest;
  if (!table) return;
  const id = newId();
  store.getState().edit("Add visual", (doc) => {
    const bottom = Math.max(0, ...doc.visuals.map((v) => v.layout.y + v.layout.h));
    return {
      ...doc,
      visuals: [
        ...doc.visuals,
        { id, spec: defaultSpec(table), layout: { x: 0, y: bottom, w: 6, h: 4 } },
      ],
    };
  });
  store.getState().setEditing(true);
  store.getState().selectVisual(id);
}
