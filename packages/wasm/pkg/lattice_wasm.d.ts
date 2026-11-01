/* tslint:disable */
/* eslint-disable */

/**
 * A uniform grid over 2-D points: each cell lists the points inside it, so a lookup scans
 * only the cells under the search window.
 */
export class PointIndex {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Points inside the rectangle (a brush).
     */
    count_in(x0: number, y0: number, x1: number, y1: number): number;
    is_empty(): boolean;
    /**
     * Indexed points.
     */
    len(): number;
    /**
     * The point closest to `(x, y)` within the ellipse of radii `rx`, `ry` (data units, so a
     * pixel radius works on axes of any scale), or -1.
     */
    nearest(x: number, y: number, rx: number, ry: number): number;
    /**
     * Indexes the points; non-finite ones are left out of every lookup.
     */
    constructor(xs: Float32Array, ys: Float32Array);
}

/**
 * Counts of `values` in `bins` equal-width bins over `[min, max]`; values outside the range
 * or not finite are ignored, and `max` itself falls in the last bin.
 */
export function histogram(values: Float64Array, bins: number, min: number, max: number): Uint32Array;

/**
 * Indices of the points LTTB keeps out of `xs`/`ys`, at most `threshold` of them, always
 * including the first and last. Returns every index when the series is already small enough.
 */
export function lttb(xs: Float64Array, ys: Float64Array, threshold: number): Uint32Array;

/**
 * [`lttb`] over the window `start..end` (a zoomed view); indices refer to the full series.
 */
export function lttb_range(xs: Float64Array, ys: Float64Array, start: number, end: number, threshold: number): Uint32Array;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_pointindex_free: (a: number, b: number) => void;
    readonly histogram: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly lttb: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly lttb_range: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
    readonly pointindex_count_in: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly pointindex_is_empty: (a: number) => number;
    readonly pointindex_len: (a: number) => number;
    readonly pointindex_nearest: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly pointindex_new: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
