"use client";

import type { ModelMetric, VisualSpec } from "@lattice/shared-types";
import { Calendar, Hash, Plus, Sigma, Type } from "lucide-react";
import { useState, type ReactNode } from "react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { CatalogTable } from "@/lib/queries";
import {
  accepts,
  draggedKind,
  droppedField,
  slotsOf,
  startDrag,
  type Field,
  type Slot,
} from "@/lib/sheets/fields";
import { cn } from "@/lib/utils";

/** What a slot is called on this kind of visual. */
export function slotLabel(spec: VisualSpec, slot: Slot): string {
  switch (slot) {
    case "axis":
      return spec.kind === "scatter"
        ? "X axis"
        : spec.kind === "pivot"
          ? "Rows"
          : spec.kind === "slicer"
            ? "Values of"
            : spec.kind === "histogram"
              ? "Values"
              : spec.kind === "pie" || spec.kind === "donut"
                ? "Slices"
                : "Group by";
    case "legend":
      return spec.kind === "pivot" ? "Columns" : spec.kind === "heatmap" ? "Y axis" : "Split by";
    case "values":
      return "Measures";
    case "drill":
      return "Drill down into";
  }
}

const NUMERIC = /^(TINYINT|SMALLINT|INTEGER|INT|BIGINT|HUGEINT|FLOAT|REAL|DOUBLE|DECIMAL|NUMERIC)/i;
const TEMPORAL = /^(DATE|TIMESTAMP)/i;

function ColumnIcon({ type }: { type: string }) {
  const Icon = TEMPORAL.test(type) ? Calendar : NUMERIC.test(type) ? Hash : Type;
  return <Icon className="size-3.5 flex-none text-subtle-foreground" aria-hidden />;
}

/** The fields a visual can use (ADR 0007): governed metrics first, then the table's columns.
 * Drag one onto a slot, or add it with its button. */
export function FieldList({
  spec,
  columns,
  metrics,
  onPlace,
}: {
  spec: VisualSpec;
  columns: CatalogTable["columns"];
  metrics: ModelMetric[];
  onPlace: (slot: Slot, field: Field) => void;
}) {
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const shownMetrics = metrics.filter((m) => m.title.toLowerCase().includes(q));
  const shownColumns = columns.filter((c) => c.name.toLowerCase().includes(q));
  const slots = slotsOf(spec);
  const row = (field: Field, label: ReactNode, icon: ReactNode, key: string) => {
    const targets = slots.filter((s) => accepts(spec, s, field));
    return (
      <li
        key={key}
        draggable
        onDragStart={(e) => startDrag(e, field)}
        className="group flex cursor-grab items-center gap-2 rounded-md px-2 py-1.5 text-[13px] hover:bg-secondary active:cursor-grabbing"
        data-field={field.kind === "metric" ? field.title : field.name}
      >
        {icon}
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {targets.length > 0 && (
          <DropdownMenu>
            <DropdownMenuTrigger
              aria-label={`Add ${field.kind === "metric" ? field.title : field.name} to…`}
              className="grid size-6 place-items-center rounded text-subtle-foreground opacity-0 group-hover:opacity-100 hover:bg-card focus-visible:opacity-100"
            >
              <Plus className="size-3.5" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {targets.map((s) => (
                <DropdownMenuItem key={s} onSelect={() => onPlace(s, field)}>
                  {slotLabel(spec, s)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </li>
    );
  };
  return (
    <section aria-label="Fields" className="grid gap-1.5">
      <input
        type="search"
        aria-label="Find a field"
        placeholder="Find a field…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        className="h-8 rounded-md border border-input bg-card px-2.5 text-[13px]"
      />
      <div className="max-h-56 overflow-y-auto rounded-lg border border-border p-1">
        {shownMetrics.length > 0 && (
          <>
            <p className="px-2 pt-1 pb-0.5 text-[11px] font-semibold tracking-wide text-subtle-foreground uppercase">
              Metrics
            </p>
            <ul>
              {shownMetrics.map((m) =>
                row(
                  { kind: "metric", id: m.note_id, title: m.title },
                  <>
                    {m.title}
                    {m.standing === "unapproved" && (
                      <span className="text-xs text-warning"> · not approved</span>
                    )}
                  </>,
                  <Sigma className="size-3.5 flex-none text-primary" aria-hidden />,
                  m.note_id,
                ),
              )}
            </ul>
          </>
        )}
        <p className="px-2 pt-1.5 pb-0.5 text-[11px] font-semibold tracking-wide text-subtle-foreground uppercase">
          Columns · not governed
        </p>
        <ul>
          {shownColumns.map((c) =>
            row(
              { kind: "column", name: c.name, dataType: c.data_type },
              c.name,
              <ColumnIcon type={c.data_type} />,
              c.name,
            ),
          )}
        </ul>
        {shownMetrics.length === 0 && shownColumns.length === 0 && (
          <p className="px-2 py-1.5 text-xs text-muted-foreground">No field matches.</p>
        )}
      </div>
    </section>
  );
}

/** A slot fields can be dropped on: it lights up while a field that fits is over it. */
export function DropSlot({
  spec,
  slot,
  onPlace,
  children,
}: {
  spec: VisualSpec;
  slot: Slot;
  onPlace: (slot: Slot, field: Field) => void;
  children: ReactNode;
}) {
  const [over, setOver] = useState(false);
  const fits = (types: readonly string[]) => {
    const kind = draggedKind(types);
    if (kind === "metric") return accepts(spec, slot, { kind, id: "", title: "" });
    // A column's type is only known on drop; placeField refuses one that doesn't fit.
    return kind === "column" && slotsOf(spec).includes(slot);
  };
  return (
    <div
      data-slot={slot}
      onDragOver={(e) => {
        if (!fits(e.dataTransfer.types)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        const field = droppedField(e.dataTransfer);
        if (!field) return;
        e.preventDefault();
        onPlace(slot, field);
      }}
      className={cn(
        "-m-1.5 rounded-lg p-1.5 transition-colors",
        over && "bg-accent ring-2 ring-primary/40",
      )}
    >
      {children}
    </div>
  );
}
