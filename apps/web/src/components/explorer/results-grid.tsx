"use client";

import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { DataType, type Field, type Table } from "apache-arrow";
import { memo, useEffect, useMemo, useRef, type CSSProperties } from "react";

import { cn } from "@/lib/utils";

const ROW_HEIGHT = 32;
const numberFormat = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

/** Formats one Arrow cell for display (dates arrive as epoch milliseconds, int64 as bigint). */
export function formatCell(value: unknown, field: Field): string {
  if (value === null || value === undefined) return "";
  if (DataType.isDate(field.type) && typeof value === "number") {
    return new Date(value).toISOString().slice(0, 10);
  }
  if (DataType.isTimestamp(field.type) && typeof value === "number") {
    return new Date(value)
      .toISOString()
      .replace("T", " ")
      .replace(/\.000Z$|Z$/, "");
  }
  if (typeof value === "number") return numberFormat.format(value);
  if (typeof value === "bigint") return value.toLocaleString("en-US");
  if (typeof value === "boolean") return value ? "true" : "false";
  return String(value);
}

function isNumeric(field: Field): boolean {
  return (
    DataType.isInt(field.type) || DataType.isFloat(field.type) || DataType.isDecimal(field.type)
  );
}

/**
 * A virtualized grid over an Arrow table. Rows are never copied into JS objects: each cell
 * reads its column vector by row index, and only the visible rows render.
 */
export const ResultsGrid = memo(function ResultsGrid({
  table,
  className,
  onNearEnd,
  labels,
  format,
  cellStyle,
}: {
  table: Table;
  /** Overrides the default size (a grid inside a sheet card fills the card). */
  className?: string;
  /** Called when the last rows scroll into view (windowed fetch of the next page). */
  onNearEnd?: () => void;
  /** Column headers in place of the result's column names. */
  labels?: string[];
  /** Formats a cell; null falls back to the default formatting. */
  format?: (value: unknown, column: number) => string | null;
  /** Conditional formatting of a cell. */
  cellStyle?: (value: unknown, column: number) => CSSProperties | undefined;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const fields = table.schema.fields;

  const columns = useMemo<ColumnDef<number>[]>(
    () =>
      fields.map((field, i) => {
        const vector = table.getChildAt(i);
        return {
          id: field.name,
          header: labels?.[i] ?? field.name,
          accessorFn: (row: number) => vector?.get(row) as unknown,
          cell: (ctx) => format?.(ctx.getValue(), i) ?? formatCell(ctx.getValue(), field),
          meta: { numeric: isNumeric(field) },
        };
      }),
    [table, fields, labels, format],
  );
  const rowIndexes = useMemo(() => Array.from({ length: table.numRows }, (_, i) => i), [table]);
  const grid = useReactTable({ data: rowIndexes, columns, getCoreRowModel: getCoreRowModel() });
  const rows = grid.getRowModel().rows;
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });
  const items = virtualizer.getVirtualItems();
  const lastVisible = items.at(-1)?.index ?? 0;
  useEffect(() => {
    if (onNearEnd && rows.length > 0 && lastVisible >= rows.length - 20) onNearEnd();
  }, [onNearEnd, lastVisible, rows.length]);
  const template = `repeat(${fields.length}, minmax(120px, 1fr))`;

  return (
    <div
      ref={scrollRef}
      role="grid"
      aria-rowcount={rows.length}
      className={cn(
        "overflow-auto rounded-[10px] border border-border bg-card text-[13px]",
        className ?? "max-h-[480px]",
      )}
    >
      <div
        role="row"
        className="sticky top-0 z-10 grid border-b border-border bg-[#fafbfc] font-medium text-secondary-foreground"
        style={{ gridTemplateColumns: template, minWidth: fields.length * 120 }}
      >
        {grid.getFlatHeaders().map((header) => (
          <div
            key={header.id}
            role="columnheader"
            className={`truncate px-3 py-2 ${(header.column.columnDef.meta as { numeric: boolean }).numeric ? "text-right" : ""}`}
          >
            {flexRender(header.column.columnDef.header, header.getContext())}
          </div>
        ))}
      </div>
      <div
        style={{
          height: virtualizer.getTotalSize(),
          position: "relative",
          minWidth: fields.length * 120,
        }}
      >
        {items.map((item) => {
          const row = rows[item.index];
          if (!row) return null;
          return (
            <div
              key={row.id}
              role="row"
              className="absolute inset-x-0 grid border-b border-border-subtle"
              style={{
                gridTemplateColumns: template,
                height: ROW_HEIGHT,
                transform: `translateY(${item.start}px)`,
              }}
            >
              {row.getVisibleCells().map((cell, column) => {
                const numeric = (cell.column.columnDef.meta as { numeric: boolean }).numeric;
                const text = flexRender(cell.column.columnDef.cell, cell.getContext());
                return (
                  <div
                    key={cell.id}
                    role="gridcell"
                    className={`truncate px-3 leading-8 ${numeric ? "text-right font-mono tabular-nums" : ""}`}
                    style={cellStyle?.(cell.getValue(), column)}
                  >
                    {text}
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
});
