# Phase 3 report — Business logic and the knowledge graph

- Date: 2026-11-02
- Exit criterion: the propose → approve → apply → re-run cycle works, with impact analysis and an
  audit log ✅ — a definition is drafted (by a person, or accepted from a profile suggestion),
  submitted, checked against its example cases, approved by its owner with the affected
  definitions and visuals listed, and every visual using it re-runs on the new version (the result
  cache keys on definition versions). Every step is in the audit log.
- Built interactively with the product owner, slice by slice; the decisions are in
  [ADR 0006](../adr/0006-knowledge-graph-and-agent-harness.md) (and [ADR 0007](../adr/0007-manual-visual-editing.md)
  for the visual editor, added at their request).

Same reference machine as before: Apple M4 (10 cores, 16 GB).

## What was built

**Knowledge graph (3.1)** — one graph per workspace, Obsidian-style: notes (entity, metric,
dimension, rule, relationship, term, fact) shared across the workspace or local to a project,
linked with `[[wiki links]]` resolved by title (unresolved links show as notes "not written yet").
Every change is a revision: draft → pending → approved, or rejected with a reason; the approved
version keeps serving while a change is drafted or reviewed; history with restore; owners (a
person and a team) approve shared notes, project admins approve project-local ones. Business
logic page: a d3-force graph (drag, zoom, hover neighbourhood, local graph, search), the note
panel, a review queue, and an editor with `[[` completion and a diff against the live version.

**Semantic layer (3.2)**
- A restricted expression language for metrics and rules
  (`SUM(order.amount) WHERE [[Settled orders only]]`, `COUNT DISTINCT(…)`, `TODAY - 90 DAYS`,
  `[[other metrics]]`), parsed and checked by the gateway, compiled to parameterised SQL — never
  passed through as text.
- The model a project sees: entities bound to its tables, many-to-one relationships (joins only
  that way, so totals can't fan out), metrics with a single grain, standing rules ("always") that
  apply once approved, dimensions, and the fiscal calendar.
- Charts use governed metrics first; raw aggregations stay, marked "Not governed"; unapproved
  logic is usable and badged "Unapproved logic". The result cache keys on every table's data
  version and every definition's version.
- Example cases (a few rows and the expected answer) run in a scratch in-memory DuckDB; a
  definition can't be approved while one fails. Reviewers see what an approval touches:
  dependent definitions and the visuals charting them.
- Fiscal quarter and year buckets, comparisons with the previous period or the same period last
  year (relative dates move back with the comparison), and fiscal year/quarter and month-to-date
  presets on date filters.

**Capture (3.3)** — profiles of connected tables suggest entities (with their key), connections of
existing entities, relationships (an `…_id` column matching another table's key) and dimensions,
each with its evidence; accepting starts a draft. Raw aggregations can be promoted to metrics
(suggested when one appears on two or more visuals); the visuals switch to the draft at once.

**Visual editor (ADR 0007)** — a global edit mode; the drawer has Data, Format and Filters tabs, a
draggable field list (metrics first), formatting (colours, labels, legend, axes, number style),
chart-level filters and Top N (ranked by category totals, so split series keep whole categories),
conditional formatting, reference lines and KPI targets.

## Numbers

Measured on the release gateway and the production Next.js build, run natively against the
development Postgres/Redis (the Compose stack would have needed a fresh database), with
Playwright's Chromium. Revenue Review now computes six of its seven visuals from governed metrics.

| Measure | Phase 3 | Phase 2 | Budget (spec §3) |
|---|---|---|---|
| Cross-filter on Revenue Review: click → every visual updated | 32.5 ms median (max 34.2) | 32.5 ms | < 150 ms |
| Longest main-thread task during those clicks | none over 50 ms | none | ≤ 50 ms |
| Switch to a prefetched sheet | 46.7 ms | 31.8 ms | < 100 ms |
| Regroup the 1M-row table over the data socket | 33.1 ms median (gateway 3.1 ms) | — | < 150 ms |
| Sign-in page first contentful paint | 28 ms | — | < 1.2 s |
| Projects page: data visible | 64.6 ms | — | < 1.2 s |
| Gateway: Revenue Review batch, uncached (11 queries over 1M rows) | 11.7 ms | 6.4–7.2 ms | — |
| Gateway: Revenue Review batch, cached | 2.5 ms | 0.9 ms | — |
| Gateway: Regional ops batch (raw columns), cached | 1.6 ms | — | — |

The extra gateway time is the metrics themselves (filtered aggregates and a distinct count of
active customers over a million orders) and loading the project's model once per batch (about
1.5 ms). The 1M-point scatter wasn't re-measured: the development database's Order explorer sheet
had been edited into a column chart; its code didn't change in this phase (phase 2: 16.8 ms p95
frame).

**Tests:** 274 Rust (unit, integration against Postgres and DuckDB, snapshot), 10 Python,
87 web unit, 31 Playwright (24 functional, 7 performance). New integration tests check compiled
metrics against hand-written SQL on the demo data, fiscal buckets against an independent
formula, Top N with split series, comparisons over relative dates, the approval gate, impact,
suggestions and promotion.

## Decisions made during the phase

All are in [ADR 0006](../adr/0006-knowledge-graph-and-agent-harness.md) (decisions 1–10 and the
slice notes) and [ADR 0007](../adr/0007-manual-visual-editing.md); in short: one graph per
workspace; owners approve; a restricted expression language; the LLM sees metadata, profiles and
samples; previous-period, year-over-year and to-date comparisons on the fiscal calendar; example
cases (rows, not live totals) gate approval; metrics first on sheets with raw columns marked;
unapproved logic usable and badged; local models by default (Qwen 3 via `mlx_lm.server`) and
local embeddings; standing rules apply once approved; a visual uses metrics or raw columns, not
both; the result cache keys on definition versions; suggestions wait in a list and become drafts
when accepted; promoted metrics switch the visuals at once; manual editing stays first-class next
to the AI.

## Deviations from the spec's phase 3 list

- **Apache AGE projection: deferred** (ADR 0006). Graph queries (links, backlinks, impact) are
  recursive SQL over the notes; nothing yet needs Cypher.
- **YAML import/export of the semantic model: not built.** The product owner set the order on
  2026-11-02: after this phase, the AI first, other features afterwards.
- **Lineage view: partly.** Approving shows the dependent definitions and the visuals charting
  them, and the graph shows every link; a column-level lineage view isn't built (same order).
- Auto-discovery bootstrap is the profile suggestions; the approval UI has the diff, example
  cases and impact.

## Open issues and follow-ups

- `a_newer_batch_cancels_the_older_one` exceeded its 5 s limit once while the machine also ran
  the dev servers and browser tests; it passes on its own and in every other full run.
- A Playwright test that times out can leave its temporary sheet in the development database
  (cleanup runs in `finally`, after the page is gone).
- "Churned customer" can't be computed: the expression language has no per-entity aggregate
  (a customer's last order date). A candidate for the language's next version.
- A cross-filter selection on a related entity's column (e.g. `customer.segment`) doesn't filter
  other visuals yet; selections still match by the visual table's own columns.
- Suggestions are worked out on every request; cache them if workspaces grow large.
- A KPI target is a number; a metric as the target would follow naturally.

## Next

Phase 4, the AI (ADR 0006 build order): 4.1 the agent harness (gateway tool server, the model loop
over the OpenAI-compatible provider with Qwen 3 on MLX, tracing, evals), 4.2 the copilot with
clarifying questions, 4.3 project → dashboard generation from the knowledge graph, 4.4 capture
from conversations and the agent's question queue. Each slice starts with its open decisions
confirmed with the product owner. Everything else waits until the AI is done.
