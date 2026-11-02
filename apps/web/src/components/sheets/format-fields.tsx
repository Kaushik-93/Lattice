"use client";

import type {
  ColorRule,
  CompareOp,
  LegendPosition,
  ReferenceKind,
  ReferenceLine,
  Visual,
  VisualSpec,
  VisualStyle,
} from "@lattice/shared-types";
import { Plus, X } from "lucide-react";
import { useId, useState, type ReactNode } from "react";

import { OTHER, PALETTE, foldSlices, shape } from "@/lib/sheets/charts";
import { useVisualResult } from "@/lib/sheets/results";
import { cn } from "@/lib/utils";

import { useSheetEnv } from "./sheet-env";

const control =
  "h-9 w-full rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";
const CARTESIAN = new Set(["line", "area", "bar", "column"]);
const RULED = new Set(["table", "pivot", "kpi"]);
const COLORS = { low: "#eef0ff", high: "#4338ca", bar: "#4338ca", alert: "#b42318" };
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
  measureNames,
  onStyle,
}: {
  visual: Visual;
  /** What each measure is called (metrics by their title). */
  measureNames: string[];
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

      {RULED.has(spec.kind) && spec.measures.length > 0 && (
        <Section title="Conditional formatting">
          <RulesEditor
            rules={style.rules ?? []}
            measureNames={measureNames}
            kpi={spec.kind === "kpi"}
            onChange={(rules, label) => onStyle(label, { rules })}
          />
        </Section>
      )}

      {spec.kind === "kpi" && (
        <Section title="Target">
          <Labeled label="Target value">
            {(id) => (
              <input
                id={id}
                type="number"
                step="any"
                className={control}
                placeholder="None"
                value={style.target ?? ""}
                onChange={(e) =>
                  onStyle(
                    "Change target",
                    { target: e.target.value === "" ? undefined : Number(e.target.value) },
                    `${visual.id}:target`,
                  )
                }
              />
            )}
          </Labeled>
        </Section>
      )}

      {CARTESIAN.has(spec.kind) && (
        <Section title="Reference lines">
          <LinesEditor
            lines={style.reference_lines ?? []}
            measureNames={spec.series ? measureNames.slice(0, 1) : measureNames}
            onChange={(reference_lines, label) => onStyle(label, { reference_lines })}
          />
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

const RULE_LABEL: Record<ColorRule["kind"], string> = {
  scale: "Colour scale",
  bars: "Data bars",
  threshold: "Highlight",
};
const OP_LABEL: Record<CompareOp, string> = {
  gt: ">",
  gte: "≥",
  lt: "<",
  lte: "≤",
  eq: "=",
};

function describeRule(rule: ColorRule, names: string[]): string {
  const of = names[rule.measure] ?? "the value";
  if (rule.kind === "threshold") return `${of} ${OP_LABEL[rule.op]} ${rule.value}`;
  return `${RULE_LABEL[rule.kind]} on ${of}`;
}

/** Conditional formatting rules: a list, and a form to add one. */
function RulesEditor({
  rules,
  measureNames,
  kpi,
  onChange,
}: {
  rules: ColorRule[];
  measureNames: string[];
  kpi: boolean;
  onChange: (rules: ColorRule[], label: string) => void;
}) {
  const kinds: ColorRule["kind"][] = kpi ? ["threshold"] : ["scale", "bars", "threshold"];
  const [kind, setKind] = useState<ColorRule["kind"]>(kinds[0]!);
  const [measure, setMeasure] = useState(0);
  const [op, setOp] = useState<CompareOp>("lt");
  const [value, setValue] = useState("");
  const [color, setColor] = useState(COLORS.alert);
  const [low, setLow] = useState(COLORS.low);
  const [high, setHigh] = useState(COLORS.high);
  const ready = kind !== "threshold" || (value.trim() !== "" && Number.isFinite(Number(value)));
  const add = () => {
    const rule: ColorRule =
      kind === "scale"
        ? { kind, measure, low, high }
        : kind === "bars"
          ? { kind, measure, color: color === COLORS.alert ? COLORS.bar : color }
          : { kind, measure, op, value: Number(value), color };
    onChange([...rules, rule], "Add formatting rule");
    setValue("");
  };
  return (
    <div className="grid gap-2">
      {rules.map((r, i) => (
        <div
          key={i}
          className="flex items-center gap-2 rounded-md bg-secondary px-2 py-1 text-[13px]"
        >
          <span
            aria-hidden
            className="size-3 flex-none rounded-sm"
            style={{
              background:
                r.kind === "scale" ? `linear-gradient(to right, ${r.low}, ${r.high})` : r.color,
            }}
          />
          <span className="min-w-0 flex-1 truncate">{describeRule(r, measureNames)}</span>
          <button
            type="button"
            aria-label={`Remove the rule ${describeRule(r, measureNames)}`}
            onClick={() =>
              onChange(
                rules.filter((_, j) => j !== i),
                "Remove formatting rule",
              )
            }
            className="grid size-6 place-items-center rounded hover:bg-card"
          >
            <X className="size-3.5 text-subtle-foreground" />
          </button>
        </div>
      ))}
      {rules.length < 10 && (
        <div className="grid gap-1.5 rounded-lg border border-border p-2">
          <div className="grid grid-cols-2 gap-1.5">
            <select
              aria-label="Rule"
              className={control}
              value={kind}
              onChange={(e) => setKind(e.target.value as ColorRule["kind"])}
            >
              {kinds.map((k) => (
                <option key={k} value={k}>
                  {RULE_LABEL[k]}
                </option>
              ))}
            </select>
            <select
              aria-label="Rule applies to"
              className={control}
              value={measure}
              onChange={(e) => setMeasure(Number(e.target.value))}
            >
              {measureNames.map((name, i) => (
                <option key={i} value={i}>
                  {name}
                </option>
              ))}
            </select>
          </div>
          {kind === "threshold" && (
            <div className="grid grid-cols-[4rem_1fr_auto] gap-1.5">
              <select
                aria-label="Comparison"
                className={control}
                value={op}
                onChange={(e) => setOp(e.target.value as CompareOp)}
              >
                {(Object.keys(OP_LABEL) as CompareOp[]).map((o) => (
                  <option key={o} value={o}>
                    {OP_LABEL[o]}
                  </option>
                ))}
              </select>
              <input
                aria-label="Compared with"
                type="number"
                step="any"
                className={control}
                value={value}
                onChange={(e) => setValue(e.target.value)}
              />
              <input
                type="color"
                aria-label="Highlight colour"
                value={color}
                onChange={(e) => setColor(e.target.value)}
                className="size-9 rounded border border-border bg-card p-0.5"
              />
            </div>
          )}
          {kind === "scale" && (
            <div className="flex items-center gap-2 text-[13px]">
              Low
              <input
                type="color"
                aria-label="Low colour"
                value={low}
                onChange={(e) => setLow(e.target.value)}
                className="size-7 rounded border border-border bg-card p-0.5"
              />
              High
              <input
                type="color"
                aria-label="High colour"
                value={high}
                onChange={(e) => setHigh(e.target.value)}
                className="size-7 rounded border border-border bg-card p-0.5"
              />
            </div>
          )}
          <button
            type="button"
            disabled={!ready}
            onClick={add}
            className="flex items-center gap-1 justify-self-start text-sm font-medium text-primary hover:underline disabled:opacity-40"
          >
            <Plus className="size-4" /> Add rule
          </button>
        </div>
      )}
    </div>
  );
}

const LINE_LABEL: Record<ReferenceKind, string> = {
  constant: "Constant",
  average: "Average",
  min: "Minimum",
  max: "Maximum",
};

/** Reference lines: a list, and a form to add one. */
function LinesEditor({
  lines,
  measureNames,
  onChange,
}: {
  lines: ReferenceLine[];
  measureNames: string[];
  onChange: (lines: ReferenceLine[], label: string) => void;
}) {
  const [kind, setKind] = useState<ReferenceKind>("average");
  const [measure, setMeasure] = useState(0);
  const [value, setValue] = useState("");
  const [label, setLabel] = useState("");
  const ready = kind !== "constant" || (value.trim() !== "" && Number.isFinite(Number(value)));
  const describe = (l: ReferenceLine) =>
    l.label ||
    (l.kind === "constant"
      ? `Line at ${l.value}`
      : `${LINE_LABEL[l.kind]} of ${measureNames[l.measure] ?? "the value"}`);
  return (
    <div className="grid gap-2">
      {lines.map((l, i) => (
        <div
          key={i}
          className="flex items-center gap-2 rounded-md bg-secondary px-2 py-1 text-[13px]"
        >
          <span className="min-w-0 flex-1 truncate">{describe(l)}</span>
          <button
            type="button"
            aria-label={`Remove the line ${describe(l)}`}
            onClick={() =>
              onChange(
                lines.filter((_, j) => j !== i),
                "Remove reference line",
              )
            }
            className="grid size-6 place-items-center rounded hover:bg-card"
          >
            <X className="size-3.5 text-subtle-foreground" />
          </button>
        </div>
      ))}
      {lines.length < 5 && (
        <div className="grid gap-1.5 rounded-lg border border-border p-2">
          <div className="grid grid-cols-2 gap-1.5">
            <select
              aria-label="Line"
              className={control}
              value={kind}
              onChange={(e) => setKind(e.target.value as ReferenceKind)}
            >
              {(Object.keys(LINE_LABEL) as ReferenceKind[]).map((k) => (
                <option key={k} value={k}>
                  {LINE_LABEL[k]}
                </option>
              ))}
            </select>
            {kind === "constant" ? (
              <input
                aria-label="Line value"
                type="number"
                step="any"
                className={control}
                value={value}
                placeholder="Value"
                onChange={(e) => setValue(e.target.value)}
              />
            ) : (
              <select
                aria-label="Line of"
                className={control}
                value={measure}
                onChange={(e) => setMeasure(Number(e.target.value))}
              >
                {measureNames.map((name, i) => (
                  <option key={i} value={i}>
                    {name}
                  </option>
                ))}
              </select>
            )}
          </div>
          <input
            aria-label="Line label"
            className={control}
            maxLength={40}
            placeholder="Label (optional)"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
          />
          <button
            type="button"
            disabled={!ready}
            onClick={() => {
              onChange(
                [
                  ...lines,
                  {
                    kind,
                    measure,
                    value: kind === "constant" ? Number(value) : undefined,
                    label: label.trim() || undefined,
                  },
                ],
                "Add reference line",
              );
              setValue("");
              setLabel("");
            }}
            className="flex items-center gap-1 justify-self-start text-sm font-medium text-primary hover:underline disabled:opacity-40"
          >
            <Plus className="size-4" /> Add line
          </button>
        </div>
      )}
    </div>
  );
}
