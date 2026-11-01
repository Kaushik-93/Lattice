import { DataType, type Table } from "apache-arrow";

function cell(value: unknown, isTemporal: boolean): string {
  if (value === null || value === undefined) return "";
  if (isTemporal && typeof value === "number") return new Date(value).toISOString().slice(0, 10);
  const text = String(value);
  return /[",\n\r]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** A result as CSV, values unformatted (dates as ISO dates). */
export function toCsv(table: Table): string {
  const fields = table.schema.fields;
  const temporal = fields.map((f) => DataType.isDate(f.type) || DataType.isTimestamp(f.type));
  const columns = fields.map((_, i) => table.getChildAt(i));
  const lines = [fields.map((f) => cell(f.name, false)).join(",")];
  for (let r = 0; r < table.numRows; r++) {
    lines.push(columns.map((c, i) => cell(c?.get(r), temporal[i] ?? false)).join(","));
  }
  return lines.join("\n") + "\n";
}

/** Saves `data` as a file through a temporary link. */
export function download(name: string, data: Blob | string): void {
  const url =
    typeof data === "string" && data.startsWith("data:")
      ? data
      : URL.createObjectURL(
          typeof data === "string" ? new Blob([data], { type: "text/csv;charset=utf-8" }) : data,
        );
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  if (!url.startsWith("data:")) setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** "Net revenue by region" → "net-revenue-by-region". */
export function fileName(title: string, extension: string): string {
  const base =
    title
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "") || "visual";
  return `${base}.${extension}`;
}
