import type { ColorRule, CompareOp } from "@lattice/shared-types";
import type { CSSProperties } from "react";

/** The lowest and highest value a measure shows, for scales and bars. */
export interface Range {
  min: number;
  max: number;
}

export function rangeOf(values: (number | null)[]): Range | null {
  const finite = values.filter((v): v is number => v !== null && Number.isFinite(v));
  if (finite.length === 0) return null;
  return { min: Math.min(...finite), max: Math.max(...finite) };
}

function rgb(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** The colour `t` (0–1) of the way from `low` to `high`. */
export function mix(low: string, high: string, t: number): string {
  const [a, b] = [rgb(low), rgb(high)];
  const k = Math.min(1, Math.max(0, t));
  const c = a.map((x, i) => Math.round(x + ((b[i] ?? x) - x) * k));
  return `#${c.map((x) => x.toString(16).padStart(2, "0")).join("")}`;
}

/** Dark or white text, whichever reads on `background`. */
function inkOn(background: string): string {
  const [r, g, b] = rgb(background).map((x) => {
    const s = x / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.4 ? "#101828" : "#ffffff";
}

export function meets(value: number, op: CompareOp, than: number): boolean {
  switch (op) {
    case "gt":
      return value > than;
    case "gte":
      return value >= than;
    case "lt":
      return value < than;
    case "lte":
      return value <= than;
    case "eq":
      return value === than;
  }
}

/** How a value of measure `measure` is painted by the rules (later rules win where they
 * overlap). `ranges` gives each measure's shown range. */
export function paint(
  rules: ColorRule[],
  measure: number,
  value: number | null,
  ranges: (Range | null)[],
): CSSProperties | undefined {
  if (value === null || !Number.isFinite(value)) return undefined;
  let style: CSSProperties | undefined;
  for (const rule of rules) {
    if (rule.measure !== measure) continue;
    const range = ranges[measure] ?? null;
    switch (rule.kind) {
      case "scale": {
        if (!range) break;
        const t = range.max === range.min ? 1 : (value - range.min) / (range.max - range.min);
        const background = mix(rule.low, rule.high, t);
        style = { ...style, backgroundColor: background, color: inkOn(background) };
        break;
      }
      case "bars": {
        if (!range || range.max <= 0) break;
        const pct = Math.max(0, Math.min(100, (value / range.max) * 100));
        style = {
          ...style,
          backgroundImage: `linear-gradient(to right, ${rule.color}40 ${pct}%, transparent ${pct}%)`,
        };
        break;
      }
      case "threshold":
        if (meets(value, rule.op, rule.value))
          style = { ...style, color: rule.color, fontWeight: 600 };
        break;
    }
  }
  return style;
}
