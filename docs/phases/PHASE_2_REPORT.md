# Phase 2 report — Visuals & sheets

- Date: 2026-11-01
- Exit criteria: 60 fps pan/zoom on 1M points ✅ (p95 frame 16.8 ms on the reference GPU) ·
  cross-filter < 150 ms ✅ (32.5 ms median, every visual of the sheet re-queried over
  1M rows) · `docker compose up` from a clean clone ✅ · CI: see the end of this report.

Same reference machine as phases 0–1: Apple M4 (10 cores, 16 GB), Docker Desktop with an 8 GB VM.
Browser numbers are from Playwright's Chromium against the production build in Compose unless
noted.

**Scope:** as agreed after phase 1, the business-logic layer (phase 3) and the AI copilot
(phase 4) are being built interactively with the product owner and were not started. Phase 2
leaves seams for them: visuals are `VisualSpec`s compiled by the gateway (what the AI will emit),
the result-cache key already carries `logic_version`, and sheet links store whether viewers may
ask the copilot. The design's copilot aside is not shown; in edit mode that panel is the visual
editor.

## What was built

**Gateway**
- Planner: raw point queries that project ≤ 4 columns may return up to 1M rows (WebGL scatter);
  numeric bins for histograms.
- `crates/visuals`: `VisualSpec` for 13 visual kinds and its compiler to table queries — sheet
  filters by column, cross-filter selections on the same table, sheet-wide slicers, drill paths
  (explicit, or finer time grains), KPI previous-period comparisons, per-kind defaults (ranked
  bars, top-12 pies, paged tables). [ADR 0005](../adr/0005-visuals-sheets-and-sharing.md).
- `crates/sheets`: sheets, visuals with grid layout, sheet filter definitions (with saved picker
  options), whole-sheet saves guarded by version, ordering, link sharing (audience, token, expiry,
  export, view tracking), sheet-only members, view records.
- API: sheet CRUD, reorder, sharing, sheet invites, project member management (add by email,
  change role, remove; owner and last admin protected), `GET /api/shared/{token}`.
- Data socket: `sheet_batch` (a sheet's visuals with its interactive state) and `shared_batch`
  (a link's saved visuals only, context narrowed to the sheet's filters and dimensions; anonymous
  sockets allowed for this message alone). Results carry a fingerprint.
- Demo seed: **Revenue Review** (KPIs with comparisons, weekly revenue, region bars with a drill
  path, top categories), **Regional ops** (slicer, heatmap, donut, histogram, monthly pivot) and
  **Order explorer** (1M-point scatter, paged order table).

**WebAssembly** (`packages/wasm`, 30 KB): LTTB downsampling, a grid point index (nearest and
brush count), histogram bins; run in a Web Worker; committed prebuilt with a source stamp.

**Web**
- Sheet page (design screen 06): tabs switched client-side with neighbouring sheets prefetched,
  header filters (date ranges with presets, value pickers), a 12-column drag-and-resize grid,
  visual cards with drill breadcrumbs, "Show data", CSV/PNG download, duplicate and delete.
- Visuals: KPI, line, area, bar (the design's ranked bars for a handful of categories), column,
  pie, donut, heatmap, histogram (ECharts, canvas, tree-shaken); pivot, slicer and virtualised
  tables with windowed paging (HTML); scatter (WebGL via regl) with pan, wheel zoom,
  shift-drag brush-to-zoom and hover through the worker's point index.
- Editing: an editor panel for type, table, fields, time grain, bins, measures, sort, limit,
  number format, stacking, KPI comparison and drill levels, plus sheet name and filters. Undo and
  redo (buttons, ⌘Z / ⇧⌘Z) over every edit, kept for the browser session; autosave with
  conflict detection.
- Members & sharing (screen 07) and the shared sheet (screen 08, `/s/<token>`).
- Project overview lists sheets with the design's cards; the sidebar lists them under Sheets.

## Numbers

### Interaction budgets (spec §3)

Measured in the page, from the state change a click causes to the frame after the last result
was rendered (`perf.spec.ts`; production build in Compose, demo data of 1M orders).

| Interaction | Median | Budget |
|---|---|---|
| Cross-filter: click a region bar → all 7 visuals of Revenue Review updated | 32.5 ms (max 34.1) | < 150 ms |
| Longest main-thread task during those clicks | none over 50 ms recorded | ≤ 50 ms |
| Switch to a prefetched sheet: tab click → its visuals settled | 31.8 ms | < 100 ms |
| Scatter, 1M points, wheel-zoom and drag-pan: p95 frame (GPU) | 16.8 ms (median 59.9 fps) | 60 fps; gated at p95 ≤ 20 ms |
| Longest main-thread task while panning and zooming | none over 50 ms recorded | ≤ 50 ms |

Development build (`make dev`, unminified React) for comparison: cross-filter 47.4 ms median,
sheet switch 78.4 ms, scatter p95 frame 16.8 ms, header region filter 66 ms.

On software WebGL (headless Chromium's default SwiftShader) the same scatter test measured a
66.7 ms p95 frame and an 84 ms long task: a million points per frame on the CPU. CI runners have
no GPU, so CI reports that number instead of gating it.

### Where cross-filter time goes

A cross-filter re-sends the whole sheet as one batch. Visuals the click doesn't affect come back
from the Redis cache with an unchanged fingerprint and are dropped before React; the affected
ones run on DuckDB in parallel. Gateway time for the whole 7-visual batch (from `batch_done`):
6.4–7.2 ms uncached (the sheet's Fiscal Q3 filter narrows 1M orders to about 92k), 0.9 ms when
every visual is a cache hit. The rest of the 32 ms is the socket, Arrow decoding and rendering.

### Other

| | |
|---|---|
| First load of Revenue Review (7 visuals, cold result cache), navigation → all visuals settled | 436 ms |
| First load of Order explorer (1M scatter points + a 500-row table), navigation → settled | 602 ms |
| WebAssembly module | 30,347 bytes |
| Images | gateway 310 MB, web 444 MB, ai 258 MB, postgres 672 MB |
| Clean clone (451 files) → `docker compose build --no-cache` | 657 s (DuckDB's C++ dominates) |
| `up --wait` until healthy, demo data and sheets seeded | 18 s |

### Tests

| Suite | Count | Result |
|---|---|---|
| Rust: unit, planner snapshots, visual compiler (11), sheets, engine, API integration incl. sheets (5) | 181 | pass |
| WebAssembly crate (native) | 5 | pass |
| Python | 10 | pass |
| Web unit (Vitest): context, drill, store/undo, chart shaping, points, results, values, phase 0–1 | 57 | pass |
| Playwright e2e + budgets: sheets (4), sheet budgets (3), phases 0–1 (12) | 19 | pass |

## Decisions made during the phase

Without asking ([ADR 0005](../adr/0005-visuals-sheets-and-sharing.md) has the reasons):
1. **The gateway compiles visual specs**, so share links can run a sheet's visuals and nothing else.
2. **Filter semantics before the semantic layer**: sheet filters and slicers by column name,
   cross-filter within a table; phase 3 relationships replace both.
3. **Whole-sheet saves with a version check**, client-generated visual ids, undo/redo as
   before/after commands in session storage.
4. **ECharts + regl + react-grid-layout**, as the spec names; the scatter's 1M-point cap is a
   planner rule for narrow point queries only.
5. **The WebAssembly package is committed prebuilt** with a source stamp, like other generated
   code; wasm-bindgen 0.2.129 (0.2.130 was two days old).
6. **Sharing** follows screens 07–08: three link audiences, plaintext 256-bit tokens (so editors
   can copy the link again), expired or switched-off links indistinguishable from unknown ones.
7. **Palette**: the design's indigo and orange plus validated reference hues; scatter colours
   capped at three, pies at eight slices.
8. **Copilot UI omitted** until phase 4; the design's right panel is the editor in edit mode.
9. **Playwright uses the GPU**; the 60 fps gate applies on hardware WebGL only.

Fixed along the way (on `main`, after the phase 1 push):
- CI's first run of the engine tests failed: parallel test processes downloaded DuckDB
  extensions into one directory at once. Installs now stage and rename atomically; DuckDB's C++
  is optimised in debug builds too; failed tests become CI annotations.
- The generated-contracts check failed only in CI: `index.ts` came from a locale-sorted glob
  (macOS put "Measure" before "MeResponse"). `gen.sh` now sorts in byte order.

## Open issues and follow-ups

1. **Anonymous link traffic** reaches the gateway; add per-link and per-IP rate limits before
   production (phase 6).
2. **Value-filter options are a snapshot** taken when the filter is added.
3. **No gateway-side M4 downsampling** (spec rule 3). Aggregated series are bounded by their time
   buckets; raw series are scatter-only, capped at 1M.
4. **Cross-filter across tables** needs relationships (phase 3).
5. **The KPI change reads "vs previous period"**, not "vs Q2": naming fiscal periods needs the
   fiscal calendar from the business-logic layer.
6. **Concurrent editing** ends in a conflict and a reload; presence and live co-editing are
   phase 6.
7. **Exports** are CSV and PNG per visual; PDF, XLSX, Parquet and project export (`.lumen`
   archives) remain (phase 5).
8. **Sheet-only invitees** reach their sheet through its link; a "Shared with me" list of sheets
   is not built yet.
9. Phase 1 issues 2–9 still stand (SSRF allow-list, MySQL untested against a server, synchronous
   uploads, no re-hydration from object storage, global memory budget, key rotation, cold image
   build time, shared CI runners).

## Next

Phase 3 (business logic) and phase 4 (AI) are for the product owner to start. Without them, the
non-AI, non-logic parts of phases 5–6 are next: warehouse connectors (Snowflake, BigQuery, SQL
Server, S3), live pushdown, incremental refresh and schedules, exports, maps, and production
concerns (Helm, scaling, presence, SSO hardening, metering).
