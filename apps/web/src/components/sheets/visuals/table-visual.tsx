"use client";

import type { Visual } from "@lattice/shared-types";
import type { Table } from "apache-arrow";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ResultsGrid } from "@/components/explorer/results-grid";
import type { QueryResult } from "@/lib/data/client";
import { measureLabel } from "@/lib/sheets/charts";
import { formatDimension, formatValue, numberStyleOf, toNumber } from "@/lib/sheets/values";

import { useSheetEnv } from "../sheet-env";

/** Rows per page the gateway returns for a raw-row table. */
const PAGE = 500;

/**
 * A virtualised table. Raw-row tables fetch further pages as the last rows scroll into view
 * (spec §2.1: windowed fetch), on a channel of their own so the sheet's batch isn't cancelled.
 */
export function TableVisual({
  visual,
  result,
  fetchPage,
}: {
  visual: Visual;
  result: QueryResult;
  fetchPage?: (offset: number) => Promise<QueryResult | null>;
}) {
  const raw = !visual.spec.x && visual.spec.measures.length === 0;
  const [table, setTable] = useState<Table>(result.table);
  const [done, setDone] = useState(!raw || !result.header.truncated);
  const loading = useRef(false);

  // A new first page (filters changed): start over from it.
  const [first, setFirst] = useState(result);
  if (first !== result) {
    setFirst(result);
    setTable(result.table);
    setDone(!raw || !result.header.truncated);
  }

  const more = useCallback(() => {
    if (done || loading.current || !fetchPage) return;
    loading.current = true;
    void fetchPage(table.numRows).then((page) => {
      loading.current = false;
      if (!page) return;
      setTable((t) => t.concat(page.table));
      setDone(!page.header.truncated || page.table.numRows < PAGE);
    });
  }, [done, fetchPage, table.numRows]);

  useEffect(() => {
    loading.current = false;
  }, [result]);

  // Summaries: readable headers and the visual's number format.
  const { fiscalStart } = useSheetEnv();
  const spec = visual.spec;
  const labels = useMemo(() => {
    if (raw) return undefined;
    const dims = [spec.x, spec.series].filter((d) => d !== undefined).map((d) => d.column);
    const fields = table.schema.fields;
    return dims.concat(spec.measures.map((m, i) => measureLabel(m, fields[dims.length + i]?.name)));
  }, [raw, spec, table]);
  const formatCellAt = useCallback(
    (value: unknown, column: number): string | null => {
      const dims = [spec.x, spec.series].filter((d) => d !== undefined);
      const dim = dims[column];
      if (dim) return formatDimension(value, table.schema.fields[column], dim, fiscalStart);
      const n = toNumber(value);
      const measure = spec.measures[column - dims.length];
      const agg = measure?.metric ? undefined : measure?.agg;
      const counted =
        agg === "count" || agg === "count_distinct" || agg === "approx_count_distinct";
      return n === null
        ? null
        : formatValue(n, counted ? "number" : (spec.options.format ?? undefined), {
            compact: false,
            ...numberStyleOf(spec.options.style),
          });
    },
    [spec, table, fiscalStart],
  );

  return (
    <div className="flex size-full min-h-0 flex-col">
      <ResultsGrid
        table={table}
        className="min-h-0 flex-1 rounded-md"
        onNearEnd={raw ? more : undefined}
        labels={labels}
        format={raw ? undefined : formatCellAt}
      />
      {raw && (
        <p className="pt-1.5 text-right text-xs text-muted-foreground" aria-live="polite">
          {table.numRows.toLocaleString("en-US")} rows{done ? "" : " loaded · scroll for more"}
        </p>
      )}
    </div>
  );
}
