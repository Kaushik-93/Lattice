import type { Filter, FilterOp, FilterValue, Scalar } from "@lattice/shared-types";

export const OPS: [FilterOp, string][] = [
  ["eq", "is"],
  ["neq", "is not"],
  ["in", "is one of"],
  ["not_in", "is none of"],
  ["contains", "contains"],
  ["gt", ">"],
  ["gte", "≥"],
  ["lt", "<"],
  ["lte", "≤"],
  ["between", "between"],
  ["is_null", "is empty"],
  ["is_not_null", "is not empty"],
];

const NUMERIC =
  /^(TINYINT|SMALLINT|INTEGER|INT|BIGINT|HUGEINT|UTINYINT|USMALLINT|UINTEGER|UBIGINT|FLOAT|REAL|DOUBLE|DECIMAL|NUMERIC)/i;

/** How many value inputs an operator takes: none, one, two (a range) or a list. */
export function arity(op: FilterOp): 0 | 1 | 2 | "list" {
  if (op === "is_null" || op === "is_not_null") return 0;
  if (op === "between") return 2;
  if (op === "in" || op === "not_in") return "list";
  return 1;
}

function scalar(text: string, dataType: string): Scalar | null {
  const t = text.trim();
  if (!t) return null;
  if (NUMERIC.test(dataType)) {
    const n = Number(t);
    return Number.isFinite(n) ? n : null;
  }
  if (/^BOOL/i.test(dataType)) return t === "true" ? true : t === "false" ? false : null;
  return t;
}

/** The filter typed into the editor, or null while it's incomplete. `text` is the value, or
 * for a list, comma-separated values; `to` is a range's upper end. */
export function filterFrom(
  column: string,
  op: FilterOp,
  dataType: string,
  text: string,
  to = "",
): Filter | null {
  if (!column) return null;
  const n = arity(op);
  if (n === 0) return { column, op, value: null };
  if (n === 2) {
    const [a, b] = [scalar(text, dataType), scalar(to, dataType)];
    return a === null || b === null ? null : { column, op, value: [a, b] };
  }
  if (n === "list") {
    const values = text
      .split(",")
      .map((s) => scalar(s, dataType))
      .filter((v): v is Scalar => v !== null);
    return values.length === 0 ? null : { column, op, value: values };
  }
  const v = scalar(text, dataType);
  return v === null ? null : { column, op, value: v };
}

/** A filter's value back as editor text: `[value, upper end]`. */
export function filterText(value: FilterValue | null | undefined, op: FilterOp): [string, string] {
  if (value === null || value === undefined) return ["", ""];
  if (Array.isArray(value)) {
    if (arity(op) === 2) return [String(value[0] ?? ""), String(value[1] ?? "")];
    return [value.map(String).join(", "), ""];
  }
  return [String(value), ""];
}

/** "region is EMEA", for a filter chip. */
export function describeFilter(f: Filter): string {
  const op = OPS.find(([o]) => o === f.op)?.[1] ?? f.op;
  const [a, b] = filterText(f.value, f.op);
  const n = arity(f.op);
  return n === 0
    ? `${f.column} ${op}`
    : n === 2
      ? `${f.column} ${op} ${a} and ${b}`
      : `${f.column} ${op} ${a}`;
}
