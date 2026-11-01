import type { NoteKind } from "@lattice/shared-types";

/** A computable note's definition as the editor shows it (docs/adr/0006). */
export interface DefinitionFields {
  formula: string;
  condition: string;
  always: boolean;
  /** "" or "1"–"12". */
  fiscalMonth: string;
  path: string;
  from: string;
  to: string;
  /** Example cases as JSON text ("" for none). */
  cases: string;
}

type Definition = Record<string, unknown>;

const text = (d: Definition | null | undefined, key: string) => {
  const v = d?.[key];
  return typeof v === "string" ? v : "";
};

export function fieldsOf(d: Definition | null | undefined): DefinitionFields {
  const month = d?.fiscal_year_start_month;
  const cases = d?.cases;
  return {
    formula: text(d, "formula"),
    condition: text(d, "condition"),
    always: d?.always === true,
    fiscalMonth: typeof month === "number" ? String(month) : "",
    path: text(d, "path"),
    from: text(d, "from"),
    to: text(d, "to"),
    cases: Array.isArray(cases) && cases.length > 0 ? JSON.stringify(cases, null, 2) : "",
  };
}

/** Which fields a kind has. */
export const FIELDS: Record<NoteKind, (keyof DefinitionFields)[]> = {
  metric: ["formula", "cases"],
  rule: ["condition", "always", "fiscalMonth", "cases"],
  dimension: ["path"],
  relationship: ["from", "to"],
  entity: [],
  term: [],
  fact: [],
};

/** The definition to save: `base` (so keys the editor doesn't know survive) with the kind's
 * fields applied. Returns an error for example cases that aren't a JSON list. */
export function definitionFrom(
  kind: NoteKind,
  f: DefinitionFields,
  base: Definition | null | undefined,
): { definition?: Definition; error?: string } {
  const d: Definition = { ...base };
  const set = (key: string, value: unknown) => {
    if (value === "" || value === false || value === undefined) delete d[key];
    else d[key] = value;
  };
  const fields = FIELDS[kind];
  if (fields.includes("formula")) set("formula", f.formula.trim());
  if (fields.includes("condition")) set("condition", f.condition.trim());
  if (fields.includes("always")) set("always", f.always);
  if (fields.includes("fiscalMonth"))
    set("fiscal_year_start_month", f.fiscalMonth ? Number(f.fiscalMonth) : undefined);
  if (fields.includes("path")) set("path", f.path.trim());
  if (fields.includes("from")) set("from", f.from.trim());
  if (fields.includes("to")) set("to", f.to.trim());
  if (fields.includes("cases")) {
    if (!f.cases.trim()) {
      delete d.cases;
    } else {
      let parsed: unknown;
      try {
        parsed = JSON.parse(f.cases);
      } catch {
        return { error: "Example cases aren't valid JSON." };
      }
      if (!Array.isArray(parsed)) return { error: "Example cases are a JSON list: [ … ]." };
      d.cases = parsed;
    }
  }
  return { definition: Object.keys(d).length > 0 ? d : undefined };
}

/** One line per part of a definition, for display and review diffs. */
export function definitionLines(
  d: Definition | null | undefined,
  { cases = true }: { cases?: boolean } = {},
): string[] {
  if (!d) return [];
  const lines: string[] = [];
  const f = fieldsOf(d);
  if (f.formula) lines.push(f.formula);
  if (f.condition) lines.push(`${f.always ? "Always: " : ""}${f.condition}`);
  if (f.fiscalMonth) lines.push(`Fiscal year starts in month ${f.fiscalMonth}`);
  if (f.path) lines.push(`Column: ${f.path}`);
  if (f.from || f.to) lines.push(`${f.from} → ${f.to} (many to one)`);
  if (cases)
    for (const c of casesOf(d)) lines.push(`Example "${c.name}": expect ${String(c.expect)}`);
  return lines;
}

export interface ExampleCase {
  name: string;
  expect: number | null;
}

export function casesOf(d: Definition | null | undefined): ExampleCase[] {
  const cases = d?.cases;
  if (!Array.isArray(cases)) return [];
  return cases.flatMap((c: unknown) => {
    if (typeof c !== "object" || c === null) return [];
    const { name, expect } = c as { name?: unknown; expect?: unknown };
    return typeof name === "string"
      ? [{ name, expect: typeof expect === "number" ? expect : null }]
      : [];
  });
}

/** A starting point for a kind's first example case. */
export function caseTemplate(kind: NoteKind): string {
  const rows = {
    order: [
      { amount: 100, status: "settled" },
      { amount: 40, status: "refunded" },
    ],
  };
  const example =
    kind === "rule"
      ? { name: "Keeps settled orders", rows, count: "order", expect: 1 }
      : { name: "Refunds don't count", rows, expect: 100 };
  return JSON.stringify([example], null, 2);
}
