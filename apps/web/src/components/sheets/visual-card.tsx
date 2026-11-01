"use client";

import type { Dimension, Scalar, Visual } from "@lattice/shared-types";
import type { Field } from "apache-arrow";
import {
  ChevronRight,
  Copy,
  Download,
  Grip,
  MoreHorizontal,
  Pencil,
  Table2,
  Trash2,
} from "lucide-react";
import { memo, useCallback, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";

import { ResultsGrid } from "@/components/explorer/results-grid";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { selectedValues } from "@/lib/sheets/context";
import {
  canDrill,
  currentLevel,
  drillLevels,
  selectionFilter,
  toggleSlicerValue,
} from "@/lib/sheets/drill";
import { download, fileName, toCsv } from "@/lib/sheets/export";
import { useVisualResult } from "@/lib/sheets/results";
import { newId } from "@/lib/sheets/store";
import { formatDate, scalarOf } from "@/lib/sheets/values";
import { cn } from "@/lib/utils";

import { useSheetEnv, useSheetState } from "./sheet-env";
import { chartImage } from "./visuals/echart";
import { ScatterGl } from "./visuals/scatter-gl";
import { TableVisual } from "./visuals/table-visual";
import { BarList, ChartVisual, KpiVisual, PivotVisual, SlicerVisual } from "./visuals/visual-body";

const NO_DRILL: Scalar[] = [];
const ECHARTS_KINDS = new Set([
  "line",
  "area",
  "bar",
  "column",
  "pie",
  "donut",
  "heatmap",
  "histogram",
]);

/** "2025" / "Q3 2025" / "Aug 2025" / "EMEA" for a value drilled into at `level`. */
function drillLabel(value: Scalar, level: Dimension | undefined): string {
  if (typeof value === "string" && level?.bucket) {
    return formatDate(Date.parse(`${value.slice(0, 10)}T00:00:00Z`), level.bucket);
  }
  return String(value);
}

/** One visual on a sheet: its card, header, drill path, menu and body. */
export const VisualCard = memo(function VisualCard({ visual }: { visual: Visual }) {
  const { store, results, canEdit, canExport, fetchPage } = useSheetEnv();
  const spec = visual.spec;
  const result = useVisualResult(results, visual.id);
  const drill = useSheetState((s) => s.drill[visual.id] ?? NO_DRILL);
  const selected = useSheetState(useShallow((s) => selectedValues(s.selections, visual.id)));
  const editing = useSheetState((s) => s.editing);
  const active = useSheetState((s) => s.selectedVisual === visual.id);
  const [asTable, setAsTable] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);

  const depth = drill.length;
  const level = currentLevel(spec, depth);
  const levels = drillLevels(spec);

  const onSelect = useCallback(
    (key: unknown, field: Field | undefined) => {
      const value = scalarOf(key, field);
      if (value === null || !level) return;
      const s = store.getState();
      if (spec.kind === "slicer") {
        const current = s.selections.find((x) => x.visual_id === visual.id)?.filter;
        const filter = toggleSlicerValue(current, level.column, value);
        s.setSelection(
          visual.id,
          filter ? { visual_id: visual.id, table_id: spec.table_id, scope: "sheet", filter } : null,
        );
        return;
      }
      s.select({
        visual_id: visual.id,
        table_id: spec.table_id,
        scope: "table",
        filter: selectionFilter(level, value),
      });
    },
    [store, spec.kind, spec.table_id, visual.id, level],
  );
  const drillable = canDrill(spec, depth);
  const onDrill = useCallback(
    (key: unknown, field: Field | undefined) => {
      const value = scalarOf(key, field);
      if (value !== null) store.getState().drillDown(visual.id, value);
    },
    [store, visual.id],
  );
  const page = useCallback(
    (offset: number) => fetchPage(visual.id, offset),
    [fetchPage, visual.id],
  );

  const remove = () =>
    store.getState().edit("Delete visual", (doc) => ({
      ...doc,
      visuals: doc.visuals.filter((v) => v.id !== visual.id),
    }));
  const duplicate = () =>
    store.getState().edit("Duplicate visual", (doc) => {
      const bottom = Math.max(0, ...doc.visuals.map((v) => v.layout.y + v.layout.h));
      const copy = {
        id: newId(),
        spec: { ...spec, title: `${spec.title} (copy)` },
        layout: { ...visual.layout, x: 0, y: bottom },
      };
      return { ...doc, visuals: [...doc.visuals, copy] };
    });
  const edit = () => {
    const s = store.getState();
    s.setEditing(true);
    s.selectVisual(visual.id);
  };

  const table = result.main?.table;
  const isKpi = spec.kind === "kpi";
  let body: React.ReactNode;
  if (result.error) {
    body = (
      <p role="alert" className="text-sm text-destructive">
        {result.error}
      </p>
    );
  } else if (!result.main || !table) {
    body = <Skeleton className="size-full min-h-10" />;
  } else if (asTable && !isKpi) {
    body = <ResultsGrid table={table} className="size-full min-h-0 rounded-md" />;
  } else {
    switch (spec.kind) {
      case "kpi":
        body = <KpiVisual visual={visual} result={result} />;
        break;
      case "bar":
        body =
          !spec.series && spec.measures.length === 1 && table.numRows <= 12 ? (
            <BarList
              visual={visual}
              table={table}
              selected={selected}
              onSelect={onSelect}
              onDrill={drillable ? onDrill : undefined}
            />
          ) : (
            <ChartVisual
              visual={visual}
              table={table}
              selected={selected}
              onSelect={onSelect}
              onDrill={drillable ? onDrill : undefined}
            />
          );
        break;
      case "scatter":
        body = <ScatterGl visual={visual} table={table} />;
        break;
      case "table":
        body = <TableVisual visual={visual} result={result.main} fetchPage={page} />;
        break;
      case "pivot":
        body = <PivotVisual visual={visual} table={table} />;
        break;
      case "slicer":
        body = (
          <SlicerVisual visual={visual} table={table} selected={selected} onSelect={onSelect} />
        );
        break;
      default:
        body = (
          <ChartVisual
            visual={visual}
            table={table}
            selected={selected}
            onSelect={spec.kind === "heatmap" || spec.kind === "histogram" ? undefined : onSelect}
            onDrill={drillable ? onDrill : undefined}
          />
        );
    }
  }

  return (
    <section
      aria-label={spec.title}
      data-visual={visual.id}
      data-kind={spec.kind}
      onClick={editing ? () => store.getState().selectVisual(visual.id) : undefined}
      className={cn(
        "group/visual flex size-full flex-col rounded-xl border border-border bg-card",
        isKpi ? "p-4 pb-3.5" : "p-4",
        editing && "cursor-default",
        active && "ring-2 ring-primary",
      )}
    >
      <header
        className={cn(
          "flex min-h-6 items-start gap-2",
          !isKpi && "mb-3",
          editing && "visual-drag cursor-move",
        )}
      >
        {editing && <Grip className="mt-0.5 size-4 flex-none text-subtle-foreground" aria-hidden />}
        <div className="min-w-0 flex-1">
          <h3
            className={cn(
              "truncate",
              isKpi
                ? "text-[13px] font-medium text-secondary-foreground"
                : "text-[15px] font-semibold",
            )}
          >
            {spec.title}
          </h3>
          {depth > 0 && (
            <nav
              aria-label="Drill path"
              className="mt-0.5 flex flex-wrap items-center gap-0.5 text-xs text-muted-foreground"
            >
              <button
                type="button"
                className="hover:text-foreground hover:underline"
                onClick={() => store.getState().drillTo(visual.id, 0)}
              >
                All
              </button>
              {drill.map((value, i) => (
                <span key={i} className="flex items-center gap-0.5">
                  <ChevronRight className="size-3" aria-hidden />
                  <button
                    type="button"
                    className="hover:text-foreground hover:underline disabled:no-underline"
                    disabled={i === depth - 1}
                    onClick={() => store.getState().drillTo(visual.id, i + 1)}
                  >
                    {drillLabel(value, levels[i])}
                  </button>
                </span>
              ))}
            </nav>
          )}
        </div>
        {result.main?.header.truncated && spec.kind !== "table" && (
          <span
            className="rounded bg-secondary px-1.5 py-0.5 text-[11px] text-muted-foreground"
            title="More rows matched than are shown"
          >
            Top {result.main.header.rows.toLocaleString("en-US")}
          </span>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger
            aria-label={`${spec.title}: more actions`}
            className="-mt-1 -mr-1 grid size-7 place-items-center rounded-md text-subtle-foreground opacity-60 group-hover/visual:opacity-100 hover:bg-secondary hover:text-foreground focus-visible:opacity-100"
          >
            <MoreHorizontal className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {canEdit && (
              <DropdownMenuItem onSelect={edit}>
                <Pencil /> Edit
              </DropdownMenuItem>
            )}
            {!isKpi && (
              <DropdownMenuItem onSelect={() => setAsTable((v) => !v)}>
                <Table2 /> {asTable ? "Show visual" : "Show data"}
              </DropdownMenuItem>
            )}
            {table && canExport && (
              <DropdownMenuItem
                onSelect={() => download(fileName(spec.title, "csv"), toCsv(table))}
              >
                <Download /> Download CSV
              </DropdownMenuItem>
            )}
            {table && canExport && ECHARTS_KINDS.has(spec.kind) && !asTable && (
              <DropdownMenuItem
                onSelect={() => {
                  const png = bodyRef.current && chartImage(bodyRef.current);
                  if (png) download(fileName(spec.title, "png"), png);
                }}
              >
                <Download /> Download PNG
              </DropdownMenuItem>
            )}
            {canEdit && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={duplicate}>
                  <Copy /> Duplicate
                </DropdownMenuItem>
                <DropdownMenuItem variant="destructive" onSelect={remove}>
                  <Trash2 /> Delete
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </header>
      <div
        ref={bodyRef}
        className="min-h-0 flex-1"
        title={drillable ? "Click to filter the sheet · double-click to drill down" : undefined}
      >
        {body}
      </div>
    </section>
  );
});
