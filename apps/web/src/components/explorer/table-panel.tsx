"use client";

import type {
  Aggregation,
  FilterOp,
  SourceColumn,
  SourceTable,
  TableQuery,
  TimeBucket,
} from "@lattice/shared-types";
import { useEffect, useId, useState } from "react";

import { Button } from "@/components/ui/button";
import { dataClient, type QueryResult } from "@/lib/data/client";
import { useResultStore } from "@/lib/data/result-store";
import { formatCount } from "@/lib/format";
import { useProfileTable } from "@/lib/queries";
import { cn } from "@/lib/utils";

import { ResultsGrid } from "./results-grid";

export function isTemporal(type: string): boolean {
  return /^(DATE|TIMESTAMP)/i.test(type);
}

export function isNumeric(type: string): boolean {
  return /^(TINYINT|SMALLINT|INTEGER|INT|BIGINT|HUGEINT|U(TINY|SMALL|BIG|HUGE)?INT|FLOAT|REAL|DOUBLE|DECIMAL|NUMERIC)/i.test(
    type,
  );
}

interface RunState {
  result: QueryResult | null;
  error: string | null;
  pending: boolean;
}

interface Builder {
  mode: "rows" | "summary";
  dimension: string;
  bucket: TimeBucket | "";
  agg: Aggregation;
  measure: string;
  filterColumn: string;
  filterOp: FilterOp;
  filterValue: string;
  limit: number;
}

const AGGREGATIONS: { value: Aggregation; label: string; numeric: boolean }[] = [
  { value: "count", label: "Count rows", numeric: false },
  { value: "sum", label: "Sum", numeric: true },
  { value: "avg", label: "Average", numeric: true },
  { value: "min", label: "Min", numeric: false },
  { value: "max", label: "Max", numeric: false },
  { value: "count_distinct", label: "Count distinct", numeric: false },
  { value: "approx_count_distinct", label: "≈ Count distinct", numeric: false },
];

function initialBuilder(table: SourceTable): Builder {
  const numeric = table.columns.find((c) => isNumeric(c.data_type))?.name ?? "";
  return {
    mode: "rows",
    dimension: table.columns.find((c) => !isNumeric(c.data_type))?.name ?? "",
    bucket: "",
    agg: numeric ? "sum" : "count",
    measure: numeric,
    filterColumn: "",
    filterOp: "eq",
    filterValue: "",
    limit: 100,
  };
}

export function toQuery(table: SourceTable, b: Builder): TableQuery {
  const filterColumn = table.columns.find((c) => c.name === b.filterColumn);
  const filters: TableQuery["filters"] = [];
  if (filterColumn && b.filterValue.trim() !== "") {
    const raw = b.filterValue.trim();
    const value =
      isNumeric(filterColumn.data_type) && !Number.isNaN(Number(raw)) ? Number(raw) : raw;
    filters.push({ column: filterColumn.name, op: b.filterOp, value });
  }
  if (b.mode === "rows") {
    return {
      table_id: table.id,
      dimensions: [],
      measures: [],
      filters,
      sort: [],
      limit: b.limit,
      offset: null,
    };
  }
  const measure = b.agg === "count" ? null : b.measure || null;
  const measureName = measure ? `${b.agg}_${measure}` : "count";
  return {
    table_id: table.id,
    dimensions: b.dimension ? [{ column: b.dimension, bucket: b.bucket || null }] : [],
    measures: [{ column: measure, agg: b.agg }],
    filters,
    // Time series read left to right; categories read biggest first.
    sort: b.bucket ? [] : [{ by: measureName, dir: "desc" }],
    limit: b.limit,
    offset: null,
  };
}

export function TablePanel({
  projectId,
  table,
  canProfile,
}: {
  projectId: string;
  table: SourceTable;
  canProfile: boolean;
}) {
  const [builder, setBuilder] = useState(() => initialBuilder(table));
  // Query state lives in a non-reactive store (the result holds an Arrow table).
  const [store, run] = useResultStore<RunState>();
  const [renderMs, setRenderMs] = useState<number>();
  const profile = useProfileTable(projectId);
  const result = run?.result ?? null;
  const error = run?.error ?? null;
  const pending = run?.pending ?? true;

  async function execute(next: Builder) {
    const previous = store.get();
    store.set({ result: previous?.result ?? null, error: null, pending: true });
    const outcome = await dataClient().run(projectId, [
      { id: "explore", query: toQuery(table, next) },
    ]);
    if (outcome.superseded) return; // a newer query is already on its way
    store.set({
      result: outcome.results.get("explore") ?? null,
      error: outcome.errors[0]?.message ?? null,
      pending: false,
    });
  }

  function update(patch: Partial<Builder>) {
    const next = { ...builder, ...patch };
    setBuilder(next);
    void execute(next);
  }

  useEffect(() => {
    void execute(builder);
    // Runs once per table (the parent keys this component by table id).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!result) return;
    const startedAt = store.updatedAt;
    const frame = requestAnimationFrame(() => setRenderMs(performance.now() - startedAt));
    return () => cancelAnimationFrame(frame);
  }, [result, store]);

  const dimension = table.columns.find((c) => c.name === builder.dimension);
  const aggInfo = AGGREGATIONS.find((a) => a.value === builder.agg);
  const measureColumns = table.columns.filter((c) => !aggInfo?.numeric || isNumeric(c.data_type));
  const unprofiled = table.columns.every((c) => !c.profile);

  return (
    <div className="flex min-w-0 flex-col gap-5">
      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card px-5 py-[18px]">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h2 className="text-[15px] font-semibold">
            {table.schema_name.startsWith("s_") ? table.name : `${table.schema_name}.${table.name}`}{" "}
            <span className="font-normal text-subtle-foreground">
              {formatCount(table.row_count)} rows · {table.columns.length} columns · {table.mode}
            </span>
          </h2>
          {canProfile && (
            <Button
              variant="outline"
              size="sm"
              disabled={profile.isPending}
              onClick={() => profile.mutate(table.id)}
            >
              {profile.isPending ? "Profiling…" : unprofiled ? "Profile columns" : "Re-profile"}
            </Button>
          )}
        </div>
        {profile.error && <p className="text-sm text-destructive">{profile.error.message}</p>}
        <ColumnList columns={table.columns} />
      </section>

      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card px-5 py-[18px]">
        <div className="flex flex-wrap items-end gap-3 text-sm">
          <Select
            label="Show"
            value={builder.mode}
            onChange={(v) => update({ mode: v as Builder["mode"] })}
            options={[
              ["rows", "Rows"],
              ["summary", "Summary"],
            ]}
          />
          {builder.mode === "summary" && (
            <>
              <Select
                label="Group by"
                value={builder.dimension}
                onChange={(v) => update({ dimension: v, bucket: "" })}
                options={[
                  ["", "Nothing (total)"],
                  ...table.columns.map((c) => [c.name, c.name] as [string, string]),
                ]}
              />
              {dimension && isTemporal(dimension.data_type) && (
                <Select
                  label="By"
                  value={builder.bucket}
                  onChange={(v) => update({ bucket: v as TimeBucket | "" })}
                  options={[
                    ["", "Exact value"],
                    ["day", "Day"],
                    ["week", "Week"],
                    ["month", "Month"],
                    ["quarter", "Quarter"],
                    ["year", "Year"],
                  ]}
                />
              )}
              <Select
                label="Measure"
                value={builder.agg}
                onChange={(v) => {
                  const agg = v as Aggregation;
                  const needsNumeric = AGGREGATIONS.find((a) => a.value === agg)?.numeric;
                  const measure =
                    needsNumeric &&
                    !isNumeric(
                      table.columns.find((c) => c.name === builder.measure)?.data_type ?? "",
                    )
                      ? (table.columns.find((c) => isNumeric(c.data_type))?.name ?? "")
                      : builder.measure || (table.columns[0]?.name ?? "");
                  update({ agg, measure });
                }}
                options={AGGREGATIONS.map((a) => [a.value, a.label] as [string, string])}
              />
              {builder.agg !== "count" && (
                <Select
                  label="of"
                  value={builder.measure}
                  onChange={(v) => update({ measure: v })}
                  options={measureColumns.map((c) => [c.name, c.name] as [string, string])}
                />
              )}
            </>
          )}
          <Select
            label="Where"
            value={builder.filterColumn}
            onChange={(v) => update({ filterColumn: v, filterValue: "" })}
            options={[
              ["", "No filter"],
              ...table.columns.map((c) => [c.name, c.name] as [string, string]),
            ]}
          />
          {builder.filterColumn && (
            <>
              <Select
                label="is"
                value={builder.filterOp}
                onChange={(v) => update({ filterOp: v as FilterOp })}
                options={[
                  ["eq", "="],
                  ["neq", "≠"],
                  ["gte", "≥"],
                  ["lte", "≤"],
                  ["contains", "contains"],
                ]}
              />
              <label className="flex flex-col gap-1">
                <span className="text-xs text-subtle-foreground">Value</span>
                <input
                  aria-label="Filter value"
                  defaultValue={builder.filterValue}
                  key={builder.filterColumn}
                  onKeyDown={(e) =>
                    e.key === "Enter" && update({ filterValue: e.currentTarget.value })
                  }
                  onBlur={(e) =>
                    e.currentTarget.value !== builder.filterValue &&
                    update({ filterValue: e.currentTarget.value })
                  }
                  className="h-9 w-36 rounded-[8px] border border-input px-2.5 text-sm"
                />
              </label>
            </>
          )}
          <Select
            label="Limit"
            value={String(builder.limit)}
            onChange={(v) => update({ limit: Number(v) })}
            options={[
              ["100", "100"],
              ["1000", "1,000"],
              ["10000", "10,000"],
              ["100000", "100,000"],
            ]}
          />
        </div>

        {error ? (
          <p
            role="alert"
            className="rounded-[10px] bg-destructive-surface px-3.5 py-2.5 text-sm text-destructive"
          >
            {error}
          </p>
        ) : result ? (
          <>
            <Timings result={result} renderMs={renderMs} pending={pending} />
            <ResultsGrid table={result.table} />
          </>
        ) : (
          <div className="h-40 animate-pulse rounded-[10px] bg-secondary" aria-busy="true" />
        )}
      </section>
    </div>
  );
}

function Select({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: [string, string][];
}) {
  // A sibling label (not a wrapping one) so the accessible name is just the label text.
  const id = useId();
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-xs text-subtle-foreground">
        {label}
      </label>
      <select
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="h-9 max-w-[200px] rounded-[8px] border border-input bg-card px-2 text-sm"
      >
        {options.map(([v, text]) => (
          <option key={v} value={v}>
            {text}
          </option>
        ))}
      </select>
    </div>
  );
}

function ColumnList({ columns }: { columns: SourceColumn[] }) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,230px),1fr))] gap-2">
      {columns.map((c) => {
        const p = c.profile;
        return (
          <div
            key={c.name}
            className="flex flex-col gap-1 rounded-[10px] border border-border-subtle bg-[#fafbfc] px-3 py-2"
          >
            <div className="flex items-baseline justify-between gap-2">
              <span className="truncate text-sm font-medium">{c.name}</span>
              <span className="shrink-0 font-mono text-[11px] text-subtle-foreground">
                {c.data_type}
              </span>
            </div>
            {p ? (
              <>
                <span className="text-xs text-muted-foreground">
                  {p.approx_distinct !== null && `≈${formatCount(p.approx_distinct)} distinct`}
                  {p.null_fraction !== null &&
                    p.null_fraction > 0 &&
                    ` · ${Math.round(p.null_fraction * 1000) / 10}% empty`}
                </span>
                {(p.min || p.max) && (
                  <span className="truncate font-mono text-[11px] text-subtle-foreground">
                    {p.min} → {p.max}
                  </span>
                )}
                {p.samples.length > 0 && (
                  <span
                    className="truncate text-[11px] text-subtle-foreground"
                    title={p.samples.join(", ")}
                  >
                    e.g. {p.samples.slice(0, 3).join(", ")}
                  </span>
                )}
              </>
            ) : (
              <span className="text-xs text-subtle-foreground">Not profiled yet</span>
            )}
          </div>
        );
      })}
    </div>
  );
}

function ms(n: number | undefined): string {
  if (n === undefined) return "…";
  return n < 10 ? n.toFixed(1) : String(Math.round(n));
}

/** Where the time went (spec §3 rule 8): gateway stages, then network, decode and render. */
function Timings({
  result,
  renderMs,
  pending,
}: {
  result: QueryResult;
  renderMs?: number;
  pending: boolean;
}) {
  const t = result.header.timings;
  const network = Math.max(0, result.clientMs - t.total_ms);
  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground transition-opacity",
        pending && "opacity-50",
      )}
      data-testid="query-timings"
    >
      <span className="font-medium text-foreground">
        {formatCount(result.header.rows)} rows{result.header.truncated && " (limit reached)"}
      </span>
      {result.header.cached && (
        <span className="rounded-full bg-success-surface px-2 py-0.5 text-success">cached</span>
      )}
      <span title="Inside the gateway: compile · cache · queue · execute · serialize">
        gateway{" "}
        <strong className="font-mono font-medium text-foreground">{ms(t.total_ms)} ms</strong>{" "}
        <span className="font-mono">
          ({ms(t.compile_ms)} · {ms(t.cache_ms)} · {ms(t.queue_ms)} · {ms(t.execute_ms)} ·{" "}
          {ms(t.serialize_ms)})
        </span>
      </span>
      <span>
        network <span className="font-mono">{ms(network)} ms</span>
      </span>
      <span>
        decode <span className="font-mono">{ms(result.decodeMs)} ms</span>
      </span>
      <span>
        render <span className="font-mono">{ms(renderMs)} ms</span>
      </span>
      <span>
        total{" "}
        <strong className="font-mono font-medium text-foreground" data-testid="query-total">
          {ms(result.clientMs + result.decodeMs + (renderMs ?? 0))} ms
        </strong>
      </span>
    </div>
  );
}
