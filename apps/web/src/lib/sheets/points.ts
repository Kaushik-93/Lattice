import { DataType, TimeUnit, type Field, type Table, type Vector } from "apache-arrow";

import { SCATTER_SLOTS } from "@/lib/sheets/charts";

/** A scatter plot's points, ready for the GPU and the point index. */
export interface Points {
  count: number;
  /** Interleaved x, y, normalised to [0, 1] (float32 keeps full precision there). */
  positions: Float32Array;
  /** Colour slot per point: 0–2, or 3 for "Other". */
  colors: Float32Array;
  /** Data range behind the normalised coordinates. */
  xRange: [number, number];
  yRange: [number, number];
  /** How to label x: raw numbers, or epoch milliseconds from `xScale`. */
  xTime: boolean;
  /** Multiply an x value by this to get epoch milliseconds (dates and timestamps). */
  xScale: number;
  /** Colour slot names, in slot order; "Other" last when values were folded. */
  categories: string[];
}

/** A numeric column straight from Arrow's buffers (1M `get()` calls would cost a frame or
 * three); nulls become NaN. */
export function numericValues(vector: Vector): Float64Array {
  const out = new Float64Array(vector.length);
  let at = 0;
  for (const data of vector.data) {
    const values = data.values as ArrayLike<number | bigint>;
    const bitmap = data.nullCount > 0 ? data.nullBitmap : undefined;
    for (let i = 0; i < data.length; i++) {
      const j = data.offset + i;
      const valid = !bitmap || ((bitmap[j >> 3] ?? 0) >> (j & 7)) & 1;
      const v = values[j];
      out[at++] = valid && v !== undefined ? Number(v) : Number.NaN;
    }
  }
  return out;
}

function timeScale(field: Field): { time: boolean; scale: number } {
  if (DataType.isDate(field.type)) {
    // Date32 holds days, Date64 milliseconds.
    return { time: true, scale: field.type.unit === 0 ? 86_400_000 : 1 };
  }
  if (DataType.isTimestamp(field.type)) {
    const per: Record<number, number> = {
      [TimeUnit.SECOND]: 1000,
      [TimeUnit.MILLISECOND]: 1,
      [TimeUnit.MICROSECOND]: 1e-3,
      [TimeUnit.NANOSECOND]: 1e-6,
    };
    return { time: true, scale: per[field.type.unit] ?? 1 };
  }
  return { time: false, scale: 1 };
}

function range(values: Float64Array): [number, number] {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  if (!Number.isFinite(lo)) return [0, 1];
  return lo === hi ? [lo - 0.5, hi + 0.5] : [lo, hi];
}

/** Columns: x, y and optionally a colour category (the gateway's projected point query). */
export function preparePoints(table: Table): Points {
  const xField = table.schema.fields[0];
  const xVector = table.getChildAt(0);
  const yVector = table.getChildAt(1);
  const count = table.numRows;
  if (!xField || !xVector || !yVector) {
    return {
      count: 0,
      positions: new Float32Array(),
      colors: new Float32Array(),
      xRange: [0, 1],
      yRange: [0, 1],
      xTime: false,
      xScale: 1,
      categories: [],
    };
  }
  const xs = numericValues(xVector);
  const ys = numericValues(yVector);
  const xRange = range(xs);
  const yRange = range(ys);
  const positions = new Float32Array(count * 2);
  const [x0, x1] = xRange;
  const [y0, y1] = yRange;
  for (let i = 0; i < count; i++) {
    positions[i * 2] = ((xs[i] ?? 0) - x0) / (x1 - x0);
    positions[i * 2 + 1] = ((ys[i] ?? 0) - y0) / (y1 - y0);
  }

  // Colour slots follow the value, sorted, never the order rows arrive in, so a filter that
  // drops a category doesn't repaint the others.
  const colors = new Float32Array(count);
  let categories: string[] = [];
  const colorVector = table.getChildAt(2);
  if (colorVector) {
    const seen = new Map<string, number>();
    const raw: string[] = new Array<string>(count);
    for (let i = 0; i < count; i++) {
      const v = colorVector.get(i);
      const name = v === null || v === undefined ? "(blank)" : String(v);
      raw[i] = name;
      seen.set(name, (seen.get(name) ?? 0) + 1);
    }
    const sorted = [...seen.keys()].sort();
    const slots = new Map<string, number>(
      sorted.map((name, i) => [name, i < SCATTER_SLOTS ? i : SCATTER_SLOTS]),
    );
    for (let i = 0; i < count; i++) colors[i] = slots.get(raw[i] ?? "") ?? SCATTER_SLOTS;
    categories = sorted.slice(0, SCATTER_SLOTS);
    if (sorted.length > SCATTER_SLOTS) categories.push("Other");
  }
  const { time, scale } = timeScale(xField);
  return { count, positions, colors, xRange, yRange, xTime: time, xScale: scale, categories };
}

/** About `target` round tick values covering [lo, hi]. */
export function niceTicks(lo: number, hi: number, target = 6): number[] {
  if (!(hi > lo)) return [lo];
  const raw = (hi - lo) / target;
  const magnitude = 10 ** Math.floor(Math.log10(raw));
  const step = ([1, 2, 2.5, 5, 10].find((m) => m * magnitude >= raw) ?? 10) * magnitude;
  const ticks: number[] = [];
  for (let i = Math.ceil(lo / step); i * step <= hi + step * 1e-9; i++) {
    // Multiplying (not accumulating) keeps ticks exact; `+ 0` turns -0 into 0.
    ticks.push(Number((i * step).toPrecision(12)) + 0);
  }
  return ticks;
}

const DAY = 86_400_000;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** Time ticks on calendar boundaries (months, or days when zoomed in). */
export function timeTicks(loMs: number, hiMs: number, target = 6): { at: number; label: string }[] {
  const spanDays = (hiMs - loMs) / DAY;
  if (spanDays <= target * 2) {
    const step = Math.max(1, Math.ceil(spanDays / target)) * DAY;
    const out: { at: number; label: string }[] = [];
    for (let t = Math.ceil(loMs / DAY) * DAY; t <= hiMs; t += step) {
      const d = new Date(t);
      out.push({ at: t, label: `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}` });
    }
    return out;
  }
  const months = Math.max(1, Math.ceil(spanDays / 30 / target));
  const step = [1, 2, 3, 6, 12, 24].find((m) => m >= months) ?? 24;
  const start = new Date(loMs);
  let y = start.getUTCFullYear();
  let m = Math.ceil((start.getUTCMonth() + (start.getUTCDate() > 1 ? 1 : 0)) / step) * step;
  const out: { at: number; label: string }[] = [];
  for (;;) {
    y += Math.floor(m / 12);
    m %= 12;
    const t = Date.UTC(y, m, 1);
    if (t > hiMs) break;
    out.push({ at: t, label: m === 0 ? String(y) : `${MONTHS[m]} ${String(y).slice(2)}` });
    m += step;
  }
  return out;
}
