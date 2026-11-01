# ADR 0005 — Visuals, sheets and sharing (phase 2)

- Status: accepted
- Date: 2026-11-01
- Evidence: [PHASE_2_REPORT](../phases/PHASE_2_REPORT.md)

## The gateway compiles visuals

A visual is stored as a `VisualSpec` (crate `visuals`): kind, table, `x` and `series`
dimensions, measures, columns, drill path, visual filters, sort, limit and display options. The
gateway compiles a spec plus the sheet's interactive state into one or more `TableQuery`s, which
the planner turns into SQL as before.

Compiling in the gateway rather than the browser is what makes share links safe: a link viewer
can only run the sheet's saved visuals, with their state narrowed to the sheet's own filters and
visual dimensions, never an arbitrary query. It also gives phase 4 one validated target (the AI
emits a `VisualSpec`, not SQL) and keeps one implementation of every rule below.

What the compiler applies:

- **Sheet filters** apply to every visual whose table has the filtered column (by name).
- **Cross-filter selections** (a clicked bar, slice or row) apply to other visuals **on the same
  table**; a visual is never filtered by its own selection (it highlights it). **Slicers** apply
  sheet-wide, by column name. Relationships between tables arrive with the business-logic layer
  (phase 3), which replaces both name-matching rules.
- **Drill paths**: explicit levels, or for a bucketed date the finer grains (year → quarter →
  month → day). Each value drilled into becomes a range (dates, bins) or equality filter.
- **KPI comparisons** re-run the KPI over the previous period of the sheet's date range:
  whole calendar months shift by months (Aug–Oct → May–Jul), other ranges by their length.

## Sheet batches on the data socket

`sheet_batch { project_id, context: { filters, selections }, visuals: [{ id, spec, drill }] }`
carries a whole sheet (spec §3 rule 7). Results are framed per visual id, plus `:compare` for a
KPI's previous period, and carry a **fingerprint** (the result-cache key's hash). The browser
keeps results in a non-reactive store per visual and drops a result whose fingerprint matches
what's shown, so a cross-filter re-renders only the visuals whose data changed; unaffected
visuals are Redis hits that never reach React. A newer batch cancels the older one end to end, as
in phase 1.

## Storage, autosave and undo

`sheets` (name, position, filter definitions, version, link settings) and `visuals` (spec,
layout). A save replaces the whole sheet in one transaction **if its version is still the one the
editor started from**; otherwise 409, and the editor offers a reload. Visual ids are generated in
the browser, so an unsaved visual already has its final id (selections, undo). A visual id
already on another sheet is refused, not taken over.

Undo/redo is a command stack in the browser (spec §8): each command holds the document before
and after, so any edit — layout, spec, filters — is exactly reversible; consecutive edits with the
same key (typing a title) merge. The stack lives in `sessionStorage`, valid while the sheet's
version is unchanged. Autosave follows 800 ms after the last edit.

Real-time co-editing is phase 6; until then, the second of two simultaneous editors gets a
conflict instead of silently losing the first one's work.

## Rendering

- **ECharts 6.1** (canvas renderer, tree-shaken to the charts used) for line, area, bar, column,
  pie, donut, heatmap and histogram. KPI cards, the design's ranked bar list, pivots, slicers
  and tables are HTML.
- **regl 2.1 (WebGL)** for scatter plots. The planner allows raw point queries that project at
  most four columns to return up to 1M rows (`MAX_POINTS`; other raw queries stay at 100k).
  Points are normalised to [0, 1] in float32 and uploaded once; pan and zoom change two
  uniforms, so a frame costs the same however the view moves. Axes are drawn on a 2-D canvas
  beneath the points in the same frame.
- **Web Worker + WebAssembly** (`packages/wasm`, spec §2.1): nearest-point hover and brush counts
  use a uniform-grid point index that lives in the worker (a hover sends two numbers, not a
  million points), and line series longer than 1,500 points are LTTB-downsampled there for the
  visible window. Gateway-side M4 downsampling (rule 3) is not built: aggregated series are
  bounded by their time buckets, and the only raw series is the scatter plot.
- **react-grid-layout 2.3** (its v2 hooks API) for the 12-column sheet grid; a drag or resize is
  one undoable command. Below 720 px visuals stack in one column.

### Colour

A categorical palette in fixed order, led by the design's indigo and orange and completed with
hues from a validated reference palette; validated as a set on the card surface (adjacent-pair
CVD ΔE ≥ 9.1, normal-vision ΔE ≥ 19.6). Four slots are under 3:1 contrast, so every visual keeps
tooltips and a "Show data" table view. Scatter plots use only the first three slots (validated
all-pairs) and fold further categories into a grey "Other"; pies fold past eight slices. Colour
slots follow the sorted value, not row order, so filtering never repaints a category. Heatmaps
use one indigo ramp, light to dark.

## The WebAssembly package is committed prebuilt

`packages/wasm/pkg` (JS glue, types, a 30 KB `.wasm`) is generated by `scripts/build-wasm.sh`
with wasm-bindgen pinned to 0.2.129 and committed, like the other generated contracts (ADR 0003):
a clean clone and the web image build need no Rust toolchain. A `SOURCE_HASH` stamp over the
crate's sources lets CI fail when the binary is stale; the algorithms are tested natively. Rebuilding
in CI and diffing bytes was rejected: release wasm can embed absolute source paths (panic
locations), so byte-identical output across machines isn't guaranteed on a stable toolchain. Turbopack bundles the `.wasm` as a hashed
static asset through the glue's `new URL(…, import.meta.url)`.

## Sharing

Design screens 07 and 08. Project members are managed by project admins (people already in the
workspace, by email; no email delivery exists yet). A sheet can have a link whose audience is
invited people (sheet-only members, signed in), anyone signed in to the workspace, or anyone; links
always grant view access to that one sheet, can expire, can allow export and count views.

- The link token is 256 random bits, stored in plaintext because the design lets editors copy
  the link at any time; rotating issues a new one, turning the link off forgets it.
- A missing, switched-off or expired link is "not found" for everyone, so links can't be probed.
- `GET /api/shared/{token}` and `shared_batch` on the socket work without a session for public
  links (the socket accepts anonymous connections for that message only). The gateway loads the
  sheet's saved visuals itself and refuses context outside the sheet's filters and dimensions.
- "Let viewers ask the copilot" is stored (off) for phase 4.

## Testing the budgets

Headless Chromium uses software WebGL (SwiftShader) unless told otherwise, which measures the CPU,
not the renderer. Playwright launches Chromium with the GPU enabled. The scatter test gates p95
frame time (a slow render stretches the frames it lands in) and skips the gate, reporting the
number, when the renderer is software, as on GPU-less CI runners. Sheet timings are measured in
the page: from the state change of a click to the frame after the last result rendered.

## Consequences

- Public links let anonymous clients reach the gateway; per-link and per-IP rate limits are a
  production requirement (phase 6).
- Values-filter options are a snapshot taken when the filter is added; new values appear after
  the filter is re-added (refreshing them belongs with scheduled refresh, phase 5).
- The design's copilot aside is not shown (phase 4); in edit mode the right-hand panel is the
  visual editor.
- Exports are CSV and PNG per visual; PDF, XLSX and Parquet are phase 5, as are maps.
