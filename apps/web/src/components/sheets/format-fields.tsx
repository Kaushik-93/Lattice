"use client";

import type { LegendPosition, Visual, VisualSpec, VisualStyle } from "@lattice/shared-types";
import { useId, type ReactNode } from "react";

import { OTHER, PALETTE, foldSlices, shape } from "@/lib/sheets/charts";
import { useVisualResult } from "@/lib/sheets/results";
import { cn } from "@/lib/utils";

import { useSheetEnv } from "./sheet-env";

const control =
  "h-9 w-full rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";
const CARTESIAN = new Set(["line", "area", "bar", "column"]);
const LEGENDS = new Set(["line", "area", "bar", "column", "pie", "donut"]);

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className="grid gap-2.5 border-t border-border-subtle pt-3 first:border-t-0 first:pt-0">
      <legend className="mb-0.5 text-xs font-semibold tracking-wide text-subtle-foreground uppercase">
        {title}
      </legend>
      {children}
    </fieldset>
  );
}

function Labeled({ label, children }: { label: string; children: (id: string) => ReactNode }) {
  const id = useId();
  return (
    <label htmlFor={id} className="grid gap-1 text-[13px] font-medium">
      {label}
      {children(id)}
    </label>
  );
}

/** The names a visual's series go by (legend entries, or a pie's slices), from its result. */
function useSeriesNames(visual: Visual): string[] {
  const { results, fiscalStart } = useSheetEnv();
  const result = useVisualResult(results, visual.id);
  const table = result.main?.table;
  if (!table || !(CARTESIAN.has(visual.spec.kind) || LEGENDS.has(visual.spec.kind))) return [];
  const shaped = shape(visual.spec, table, fiscalStart);
  if (visual.spec.kind === "pie" || visual.spec.kind === "donut") {
    return foldSlices(shaped).labels.filter((l) => l !== "Other");
  }
  return shaped.series.map((s) => s.name);
}

/** The Format tab (ADR 0007): how a visual looks, never what it computes. */
export function FormatFields({
  visual,
  onStyle,
}: {
  visual: Visual;
  /** Merges `patch` into the style; `merge` coalesces typing into one undo step. */
  onStyle: (label: string, patch: Partial<VisualStyle>, merge?: string) => void;
}) {
  const spec: VisualSpec = visual.spec;
  const style: VisualStyle = spec.options.style ?? {};
  const names = useSeriesNames(visual);
  const text = (key: keyof VisualStyle, label: string, max: number, placeholder?: string) => (
    <Labeled label={label}>
      {(id) => (
        <input
          id={id}
          className={control}
          maxLength={max}
          placeholder={placeholder}
          value={(style[key] as string | undefined) ?? ""}
          onChange={(e) =>
            onStyle(
              `Change ${label.toLowerCase()}`,
              { [key]: e.target.value || undefined },
              `${visual.id}:${key}`,
            )
          }
        />
      )}
    </Labeled>
  );
  const bound = (key: "y_min" | "y_max", label: string) => (
    <Labeled label={label}>
      {(id) => (
        <input
          id={id}
          type="number"
          step="any"
          className={control}
          placeholder="Auto"
          value={style[key] ?? ""}
          onChange={(e) =>
            onStyle(
              `Change ${label.toLowerCase()}`,
              { [key]: e.target.value === "" ? undefined : Number(e.target.value) },
              `${visual.id}:${key}`,
            )
          }
        />
      )}
    </Labeled>
  );
  const check = (key: "data_labels" | "y_log" | "gridlines", label: string, fallback: boolean) => (
    <label className="flex items-center gap-2 text-[13px]">
      <input
        type="checkbox"
        checked={style[key] ?? fallback}
        onChange={(e) =>
          onStyle(`Toggle ${label.toLowerCase()}`, {
            [key]: e.target.checked === fallback ? undefined : e.target.checked,
          })
        }
      />
      {label}
    </label>
  );

  return (
    <div className="grid gap-4">
      {spec.kind !== "slicer" && spec.kind !== "scatter" && (
        <Section title="Numbers">
          <div className="grid grid-cols-3 gap-2">
            <Labeled label="Decimals">
              {(id) => (
                <select
                  id={id}
                  className={control}
                  value={style.decimals ?? ""}
                  onChange={(e) =>
                    onStyle("Change decimals", {
                      decimals: e.target.value === "" ? undefined : Number(e.target.value),
                    })
                  }
                >
                  <option value="">Auto</option>
                  {[0, 1, 2, 3, 4].map((d) => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
              )}
            </Labeled>
            {text("prefix", "Prefix", 8, "e.g. ~")}
            {text("suffix", "Suffix", 8, "e.g. units")}
          </div>
        </Section>
      )}

      {LEGENDS.has(spec.kind) && (
        <Section title="Labels and legend">
          {check("data_labels", "Show data labels", false)}
          <Labeled label="Legend">
            {(id) => (
              <select
                id={id}
                className={control}
                value={style.legend ?? ""}
                onChange={(e) =>
                  onStyle("Move legend", {
                    legend: (e.target.value || undefined) as LegendPosition | undefined,
                  })
                }
              >
                <option value="">Automatic</option>
                <option value="top">Top</option>
                <option value="bottom">Bottom</option>
                <option value="right">Right</option>
                <option value="hidden">Hidden</option>
              </select>
            )}
          </Labeled>
        </Section>
      )}

      {CARTESIAN.has(spec.kind) && (
        <Section title="Axes">
          {text("x_title", "Category axis title", 80)}
          {text("y_title", "Value axis title", 80)}
          <div className="grid grid-cols-2 gap-2">
            {bound("y_min", "Value axis minimum")}
            {bound("y_max", "Value axis maximum")}
          </div>
          {check("y_log", "Logarithmic scale", false)}
          {check("gridlines", "Gridlines", true)}
        </Section>
      )}

      {names.length > 0 && (
        <Section title="Colours">
          <ul className="grid gap-1.5">
            {names.map((name, i) => {
              const picked = style.colors?.[name];
              const color = picked ?? PALETTE[i] ?? OTHER;
              return (
                <li key={name} className="flex items-center gap-2 text-[13px]">
                  <input
                    type="color"
                    aria-label={`Colour of ${name}`}
                    value={color}
                    onChange={(e) =>
                      onStyle(
                        "Change colour",
                        { colors: { ...style.colors, [name]: e.target.value } },
                        `${visual.id}:color:${name}`,
                      )
                    }
                    className="size-7 flex-none cursor-pointer rounded border border-border bg-card p-0.5"
                  />
                  <span className="min-w-0 flex-1 truncate">{name}</span>
                  <button
                    type="button"
                    disabled={!picked}
                    onClick={() => {
                      const rest = Object.fromEntries(
                        Object.entries(style.colors ?? {}).filter(([k]) => k !== name),
                      );
                      onStyle("Reset colour", { colors: rest });
                    }}
                    className={cn("text-xs text-primary hover:underline disabled:invisible")}
                  >
                    Reset
                  </button>
                </li>
              );
            })}
          </ul>
        </Section>
      )}
    </div>
  );
}

/** `style` with `patch` applied, dropping what went back to its default. */
export function patchStyle(style: VisualStyle, patch: Partial<VisualStyle>): VisualStyle {
  const next: Record<string, unknown> = { ...style, ...patch };
  for (const [k, v] of Object.entries(next)) {
    const empty =
      v === undefined ||
      v === "" ||
      (typeof v === "object" && v !== null && Object.keys(v).length === 0);
    if (empty) delete next[k];
  }
  return next as VisualStyle;
}
