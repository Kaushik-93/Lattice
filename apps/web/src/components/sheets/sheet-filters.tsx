"use client";

import type { DatePreset, FilterValue, Scalar, SheetFilterDef } from "@lattice/shared-types";
import { Calendar, ChevronDown } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useShallow } from "zustand/react/shallow";

import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { currentValue } from "@/lib/sheets/context";
import { PRESET_LABEL, presetRange } from "@/lib/sheets/fiscal";
import type { SheetStore } from "@/lib/sheets/store";
import { cn } from "@/lib/utils";

import { useSheetEnv, useSheetState } from "./sheet-env";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const trigger =
  "inline-flex h-10 items-center gap-2 rounded-[10px] border border-input bg-card px-3.5 text-sm font-medium text-foreground hover:bg-secondary focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

function asList(value: FilterValue | null): Scalar[] {
  if (value === null) return [];
  return Array.isArray(value) ? value : [value];
}

/** "Aug 1 – Oct 31, 2026". */
export function rangeLabel(from: string, to: string): string {
  const [fy, fm, fd] = from.split("-").map(Number);
  const [ty, tm, td] = to.split("-").map(Number);
  const f = `${MONTHS[(fm ?? 1) - 1]} ${fd}`;
  const t = `${MONTHS[(tm ?? 1) - 1]} ${td}, ${ty}`;
  return fy === ty ? `${f} – ${t}` : `${f}, ${fy} – ${t}`;
}

function isoDaysAgo(days: number): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() - days);
  return d.toISOString().slice(0, 10);
}

/** The sheet's filters as header controls (design screen 06), or as the segmented region
 * picker of a shared sheet (screen 08). */
export function SheetFilters({ segmented = false }: { segmented?: boolean }) {
  const filters = useSheetState((s) => s.doc.filters);
  return (
    <>
      {filters.map((def) =>
        def.kind === "date_range" ? (
          <DateRangeFilter key={def.id} def={def} />
        ) : segmented && (def.options?.length ?? 0) <= 6 ? (
          <SegmentedFilter key={def.id} def={def} />
        ) : (
          <ValuesFilter key={def.id} def={def} />
        ),
      )}
    </>
  );
}

function DateRangeFilter({ def }: { def: SheetFilterDef }) {
  const value = useSheetState(useShallow((s) => asList(currentValue(def, s.filterValues))));
  const setFilterValue = useSheetState((s) => s.setFilterValue);
  const [open, setOpen] = useState(false);
  const fromId = useId();
  const toId = useId();
  const [from, to] = value.map(String);
  const fiscalStart = useSheetEnv().fiscalStart;
  const defaultValue = def.preset ? presetRange(def.preset, fiscalStart) : (def.value ?? null);
  const isDefault = JSON.stringify(value) === JSON.stringify(asList(defaultValue));
  const label = !from || !to ? "All dates" : isDefault ? def.label : rangeLabel(from, to);
  const today = new Date().toISOString().slice(0, 10);
  const set = (v: FilterValue | null) => {
    setFilterValue(def.id, v);
    setOpen(false);
  };
  const presets: [string, FilterValue | null][] = [
    ...(defaultValue && !def.preset ? [[def.label, defaultValue] as [string, FilterValue]] : []),
    ...(Object.keys(PRESET_LABEL) as DatePreset[]).map(
      (p) => [PRESET_LABEL[p], presetRange(p, fiscalStart)] as [string, FilterValue],
    ),
    ["Last 30 days", [isoDaysAgo(29), today]],
    ["Last 90 days", [isoDaysAgo(89), today]],
    ["All dates", null],
  ];
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className={trigger} aria-label={`${def.column} range: ${label}`}>
        <Calendar className="size-4 text-subtle-foreground" aria-hidden />
        {label}
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80">
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            const data = new FormData(e.currentTarget);
            const a = String(data.get("from") ?? "");
            const b = String(data.get("to") ?? "");
            if (a && b) set(a <= b ? [a, b] : [b, a]);
          }}
        >
          <div className="flex flex-wrap gap-1.5">
            {presets.map(([name, v]) => (
              <button
                key={name}
                type="button"
                onClick={() => set(v)}
                className="rounded-full border border-border px-2.5 py-1 text-xs hover:border-primary hover:text-accent-foreground"
              >
                {name}
              </button>
            ))}
          </div>
          <div className="grid grid-cols-2 gap-2">
            <label
              htmlFor={fromId}
              className="grid gap-1 text-xs font-medium text-secondary-foreground"
            >
              From
              <input
                id={fromId}
                name="from"
                type="date"
                defaultValue={from}
                className="h-9 rounded-md border border-input px-2 text-sm"
              />
            </label>
            <label
              htmlFor={toId}
              className="grid gap-1 text-xs font-medium text-secondary-foreground"
            >
              To
              <input
                id={toId}
                name="to"
                type="date"
                defaultValue={to}
                className="h-9 rounded-md border border-input px-2 text-sm"
              />
            </label>
          </div>
          <button
            type="submit"
            className="h-9 rounded-md bg-primary text-sm font-semibold text-primary-foreground hover:bg-primary-hover"
          >
            Apply
          </button>
        </form>
      </PopoverContent>
    </Popover>
  );
}

function pluralLabel(def: SheetFilterDef): string {
  const word = def.label.toLowerCase();
  return word.endsWith("s") ? word : `${word}s`;
}

function ValuesFilter({ def }: { def: SheetFilterDef }) {
  const picked = useSheetState(useShallow((s) => asList(currentValue(def, s.filterValues))));
  const setFilterValue = useSheetState((s) => s.setFilterValue);
  const label =
    picked.length === 0
      ? `All ${pluralLabel(def)}`
      : picked.length === 1
        ? String(picked[0])
        : `${picked.length} ${pluralLabel(def)}`;
  const toggle = (option: Scalar) => {
    const next = picked.includes(option) ? picked.filter((p) => p !== option) : [...picked, option];
    setFilterValue(def.id, next.length ? next : null);
  };
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className={trigger} aria-label={`${def.label}: ${label}`}>
        {label}
        <ChevronDown className="size-4 text-subtle-foreground" aria-hidden />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-48">
        <DropdownMenuItem onSelect={() => setFilterValue(def.id, null)}>
          All {pluralLabel(def)}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        {(def.options ?? []).map((option) => (
          <DropdownMenuCheckboxItem
            key={String(option)}
            checked={picked.includes(option)}
            onSelect={(e) => e.preventDefault()}
            onCheckedChange={() => toggle(option)}
          >
            {String(option)}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** "Region: All | AMER | EMEA | APAC" (design screen 08). */
function SegmentedFilter({ def }: { def: SheetFilterDef }) {
  const picked = useSheetState(useShallow((s) => asList(currentValue(def, s.filterValues))));
  const setFilterValue = useSheetState((s) => s.setFilterValue);
  const current = picked.length === 1 ? picked[0] : null;
  const options: (Scalar | null)[] = [null, ...(def.options ?? [])];
  return (
    <div className="flex items-center gap-3">
      <span className="text-sm text-secondary-foreground">{def.label}</span>
      <div
        role="radiogroup"
        aria-label={`${def.label} filter`}
        className="flex rounded-[10px] bg-secondary p-1"
      >
        {options.map((option) => {
          const checked = option === current || (option === null && picked.length === 0);
          return (
            <button
              key={String(option)}
              type="button"
              role="radio"
              aria-checked={checked}
              onClick={() => setFilterValue(def.id, option === null ? null : [option])}
              className={cn(
                "h-8 rounded-lg px-3.5 text-sm font-medium",
                checked
                  ? "bg-card text-foreground shadow-sm"
                  : "text-secondary-foreground hover:text-foreground",
              )}
            >
              {option === null ? "All" : String(option)}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** Date filters whose default is "to date" start from today's range when the sheet opens, once
 * the fiscal calendar is known. */
export function usePresetDefaults(store: SheetStore, fiscalStart: number | undefined) {
  useEffect(() => {
    if (fiscalStart === undefined) return;
    const s = store.getState();
    for (const def of s.doc.filters) {
      if (def.preset && !(def.id in s.filterValues)) {
        s.setFilterValue(def.id, presetRange(def.preset, fiscalStart));
      }
    }
  }, [store, fiscalStart]);
}
