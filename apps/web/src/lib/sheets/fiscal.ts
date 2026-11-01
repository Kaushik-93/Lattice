import type { DatePreset } from "@lattice/shared-types";

/**
 * The fiscal calendar (docs/adr/0006): the year starts in `start` (1 = January) and is named
 * after the calendar year it ends in, so with a February start, FY2027 runs Feb 2026–Jan 2027.
 */

/** The fiscal year a month (0-based) of a calendar year falls in. */
function fiscalYear(year: number, month0: number, start: number): number {
  if (start <= 1) return year;
  return month0 >= start - 1 ? year + 1 : year;
}

/** "Q3 FY2027" for the fiscal quarter starting on that date. */
export function fiscalQuarterLabel(ms: number, start: number): string {
  const d = new Date(ms);
  const [y, m] = [d.getUTCFullYear(), d.getUTCMonth()];
  const quarter = Math.floor(((m - (start - 1) + 12) % 12) / 3) + 1;
  return `Q${quarter} FY${fiscalYear(y, m, start)}`;
}

/** "FY2027" for the fiscal year starting on that date. */
export function fiscalYearLabel(ms: number, start: number): string {
  const d = new Date(ms);
  return `FY${fiscalYear(d.getUTCFullYear(), d.getUTCMonth(), start)}`;
}

const iso = (y: number, m0: number, day: number) =>
  new Date(Date.UTC(y, m0, day)).toISOString().slice(0, 10);

/** First day of the period of `months` (3 or 12) on the fiscal calendar that holds `today`. */
function periodStart(today: Date, months: number, start: number): string {
  const index = today.getFullYear() * 12 + today.getMonth() - (start - 1);
  const first = Math.floor(index / months) * months + (start - 1);
  return iso(Math.floor(first / 12), first % 12, 1);
}

/** A preset's `[from, to]`, ending today (the viewer's local date). */
export function presetRange(
  preset: DatePreset,
  start: number,
  today = new Date(),
): [string, string] {
  const end = iso(today.getFullYear(), today.getMonth(), today.getDate());
  switch (preset) {
    case "fiscal_year_to_date":
      return [periodStart(today, 12, start), end];
    case "fiscal_quarter_to_date":
      return [periodStart(today, 3, start), end];
    case "month_to_date":
      return [iso(today.getFullYear(), today.getMonth(), 1), end];
  }
}

export const PRESET_LABEL: Record<DatePreset, string> = {
  fiscal_year_to_date: "Fiscal year to date",
  fiscal_quarter_to_date: "Fiscal quarter to date",
  month_to_date: "Month to date",
};
