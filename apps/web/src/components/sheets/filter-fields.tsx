"use client";

import type { Filter, FilterOp, TopN, VisualSpec } from "@lattice/shared-types";
import { Plus, X } from "lucide-react";
import { useId, useState } from "react";

import type { CatalogTable } from "@/lib/queries";
import { OPS, arity, describeFilter, filterFrom, filterText } from "@/lib/sheets/filters";

const control =
  "h-9 w-full min-w-0 rounded-md border border-input bg-card px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";
const RANKED = new Set([
  "bar",
  "column",
  "line",
  "area",
  "pie",
  "donut",
  "table",
  "pivot",
  "heatmap",
]);

/** One chart filter being written or changed: committed with Apply once complete. */
function FilterForm({
  initial,
  columns,
  submit,
  onSubmit,
  onCancel,
}: {
  initial: Filter | null;
  columns: CatalogTable["columns"];
  submit: string;
  onSubmit: (f: Filter) => void;
  onCancel?: () => void;
}) {
  const id = useId();
  const [column, setColumn] = useState(initial?.column ?? columns[0]?.name ?? "");
  const [op, setOp] = useState<FilterOp>(initial?.op ?? "eq");
  const [[text, to], setText] = useState(filterText(initial?.value, initial?.op ?? "eq"));
  const dataType = columns.find((c) => c.name === column)?.data_type ?? "";
  const date = /^(DATE|TIMESTAMP)/i.test(dataType);
  const filter = filterFrom(column, op, dataType, text, to);
  const n = arity(op);
  const input = (value: string, set: (v: string) => void, label: string) => (
    <input
      aria-label={label}
      type={date && n !== "list" ? "date" : "text"}
      className={control}
      value={value}
      placeholder={n === "list" ? "A, B, C" : "Value"}
      onChange={(e) => set(e.target.value)}
    />
  );
  return (
    <form
      className="grid gap-1.5 rounded-lg border border-border p-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (filter) onSubmit(filter);
      }}
    >
      <div className="grid grid-cols-[1fr_auto] gap-1.5">
        <select
          aria-label="Filter column"
          id={`${id}-column`}
          className={control}
          value={column}
          onChange={(e) => setColumn(e.target.value)}
        >
          {columns.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
            </option>
          ))}
        </select>
        <select
          aria-label="Filter operator"
          className={control}
          value={op}
          onChange={(e) => setOp(e.target.value as FilterOp)}
        >
          {OPS.map(([o, label]) => (
            <option key={o} value={o}>
              {label}
            </option>
          ))}
        </select>
      </div>
      {n !== 0 && (
        <div className={n === 2 ? "grid grid-cols-2 gap-1.5" : "grid"}>
          {input(text, (v) => setText([v, to]), n === 2 ? "From" : "Filter value")}
          {n === 2 && input(to, (v) => setText([text, v]), "To")}
        </div>
      )}
      <div className="flex justify-end gap-2">
        {onCancel && (
          <button
            type="button"
            onClick={onCancel}
            className="text-xs text-subtle-foreground hover:text-foreground"
          >
            Cancel
          </button>
        )}
        <button
          type="submit"
          disabled={!filter}
          className="h-7 rounded-md bg-primary px-2.5 text-xs font-semibold text-primary-foreground disabled:opacity-40"
        >
          {submit}
        </button>
      </div>
    </form>
  );
}

/** The Filters tab (ADR 0007): filters for this chart only, and Top N. */
export function FilterFields({
  spec,
  columns,
  measureNames,
  onChange,
}: {
  spec: VisualSpec;
  columns: CatalogTable["columns"];
  /** What each measure is called (metrics by their title). */
  measureNames: string[];
  onChange: (label: string, patch: (s: VisualSpec) => VisualSpec) => void;
}) {
  const [editing, setEditing] = useState<number | "new" | null>(null);
  const ranked = RANKED.has(spec.kind) && !!spec.x && spec.measures.length > 0;
  const top = spec.top ?? null;
  const setTop = (t: TopN | null) =>
    onChange(t ? "Change top N" : "Remove top N", (s) => ({ ...s, top: t ?? undefined }));
  return (
    <div className="grid gap-4">
      <fieldset className="grid gap-1.5">
        <legend className="mb-1 text-xs font-semibold text-secondary-foreground">
          Filters on this chart
        </legend>
        {spec.filters.length === 0 && editing !== "new" && (
          <p className="text-[13px] text-muted-foreground">
            None. The sheet&apos;s filters still apply.
          </p>
        )}
        {spec.filters.map((f, i) =>
          editing === i ? (
            <FilterForm
              key={i}
              initial={f}
              columns={columns}
              submit="Apply"
              onCancel={() => setEditing(null)}
              onSubmit={(next) => {
                onChange("Change chart filter", (s) => ({
                  ...s,
                  filters: s.filters.map((x, j) => (j === i ? next : x)),
                }));
                setEditing(null);
              }}
            />
          ) : (
            <div
              key={i}
              className="flex items-center gap-1 rounded-md bg-secondary px-2 py-1 text-[13px]"
            >
              <button
                type="button"
                className="min-w-0 flex-1 truncate text-left hover:underline"
                onClick={() => setEditing(i)}
              >
                {describeFilter(f)}
              </button>
              <button
                type="button"
                aria-label={`Remove the filter ${describeFilter(f)}`}
                onClick={() =>
                  onChange("Remove chart filter", (s) => ({
                    ...s,
                    filters: s.filters.filter((_, j) => j !== i),
                  }))
                }
                className="grid size-6 place-items-center rounded hover:bg-card"
              >
                <X className="size-3.5 text-subtle-foreground" />
              </button>
            </div>
          ),
        )}
        {editing === "new" ? (
          <FilterForm
            initial={null}
            columns={columns}
            submit="Add filter"
            onCancel={() => setEditing(null)}
            onSubmit={(f) => {
              onChange("Add chart filter", (s) => ({ ...s, filters: [...s.filters, f] }));
              setEditing(null);
            }}
          />
        ) : (
          spec.filters.length < 20 && (
            <button
              type="button"
              onClick={() => setEditing("new")}
              className="flex items-center gap-1.5 text-sm font-medium text-primary hover:underline"
            >
              <Plus className="size-4" /> Add a filter
            </button>
          )
        )}
      </fieldset>

      {ranked && (
        <fieldset className="grid gap-1.5">
          <legend className="mb-1 text-xs font-semibold text-secondary-foreground">Top N</legend>
          <label className="flex items-center gap-2 text-[13px]">
            <input
              type="checkbox"
              checked={top !== null}
              onChange={(e) => setTop(e.target.checked ? { n: 10, bottom: false, by: 0 } : null)}
            />
            Show only the top or bottom {spec.x?.column ?? "values"}
          </label>
          {top && (
            <div className="grid grid-cols-[auto_5rem_1fr] items-center gap-1.5 text-[13px]">
              <select
                aria-label="Top or bottom"
                className={control}
                value={top.bottom ? "bottom" : "top"}
                onChange={(e) => setTop({ ...top, bottom: e.target.value === "bottom" })}
              >
                <option value="top">Top</option>
                <option value="bottom">Bottom</option>
              </select>
              <input
                aria-label="How many"
                type="number"
                min={1}
                max={1000}
                className={control}
                value={top.n}
                onChange={(e) => {
                  const n = Math.round(Number(e.target.value));
                  if (n >= 1 && n <= 1000) setTop({ ...top, n });
                }}
              />
              <select
                aria-label="Ranked by"
                className={control}
                value={top.by}
                onChange={(e) => setTop({ ...top, by: Number(e.target.value) })}
              >
                {measureNames.map((name, i) => (
                  <option key={i} value={i}>
                    by {name}
                  </option>
                ))}
              </select>
            </div>
          )}
        </fieldset>
      )}
    </div>
  );
}
