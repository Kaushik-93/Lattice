# ADR 0006 — The knowledge graph and the agent harness (phases 3–4)

- Status: accepted (direction and decisions from the product owner, 2026-11-01)
- Amends: spec §6 (business-logic layer) and §7 (AI pipeline); builds on ADR 0002 (layering,
  roles) and ADR 0005 (visual specs compiled by the gateway).

## Direction

The business-logic layer is a **knowledge graph that learns**, in the spirit of an Obsidian
vault: every piece of business knowledge is a linked note, the graph grows as people connect
sources, build sheets, answer the copilot's questions and correct it, and nothing becomes truth
until a person approves it. When someone creates a project, the knowledge relevant to it is
gathered from the graph and given to the model that drafts its dashboards. The AI side is
**harness-engineered**: typed tools, deterministic compilation, validation, self-checks, approval
gates, evals and traces, so the system can be relied on — and when it isn't sure, it asks.

## Decisions

1. **One graph per workspace** (one vault per company). Notes are shared across the workspace or
   local to one project; a project sees the shared notes plus its own. (ADR 0002's layering.)
2. **Owners approve.** Each shared note has an owner (a person, labelled with a team such as
   "Finance"); the owner or a workspace admin approves changes to it. Project admins approve
   project-local notes. Approval stays a separate, recorded act even for one person holding
   every role.
3. **Formulas use a restricted expression language** — e.g.
   `SUM(order.amount) - SUM(refund.amount) WHERE order.status = 'settled'` — parsed and checked
   by the gateway, compiled to SQL, written by people and the AI alike. No raw SQL in definitions.
4. **The LLM sees metadata, profiles and samples**: names, descriptions, column statistics and a
   few sample values. It never receives full query results; its preview tool returns shapes and
   sanity statistics, not rows.

Decided for slice 3.2 (2026-11-01):

5. **Time comparisons**: previous period, same period last year, and year/quarter/month to
   date, all on the fiscal calendar note.
6. **Example cases gate approval**: a definition can't be approved while any of its example
   cases fails; the reviewer sees which and why.
7. **Sheets offer governed metrics first**; raw column aggregations stay available for
   exploration, marked as not governed.
8. **Unapproved metrics may be used on sheets, badged** "Uses unapproved logic" with a link to
   the proposal (spec §6.3); the badge goes when the logic is approved.

9. **Local models by default** (amends spec §2.3, whose default is Claude): the AI service
   talks to an OpenAI-compatible endpoint, served in development by `mlx_lm.server` running
   Qwen 3 natively on the Mac (MLX can't run inside Docker; the AI container reaches it at
   `host.docker.internal`), and on Linux servers by vLLM, llama.cpp or Ollama. Hosted
   providers (Claude) stay pluggable behind the same `LlmProvider` protocol. On the 16 GB
   reference machine a 4-bit Qwen3-8B fits comfortably; which model is good enough is
   decided by the evals, not assumed.
10. **Embeddings are computed locally** by a small open embedding model in the AI service, so
    knowledge retrieval sends nothing off the machine.

## The graph

- **Notes** have a kind — entity, metric, dimension, rule, relationship, term, fact — a title, a
  markdown body with `[[links]]` to other notes (unresolved links are allowed and shown, as in
  Obsidian), and for computable kinds a structured definition. Physical nodes (sources, tables,
  columns) and usage nodes (sheets, visuals) come from the existing catalog and sheets; they are
  not duplicated as notes.
- **Lifecycle per revision**: a note's live content is its latest *approved* revision. Any change
  is a new revision that moves **draft → pending → approved** (or **rejected**, with a reason that
  is kept and fed back to the AI); a note can be **deprecated**. So an approved metric keeps
  serving while a proposed change to it waits, and every version stays diffable and revertible.
- **Provenance on every revision**: who or what proposed it (a person, the profiler, a sheet,
  a conversation), the evidence (a query and summary of its result), and a confidence.
- **Storage**: relational tables in Postgres are the source of truth (spec §6.2): notes,
  revisions and the links extracted from each revision. Neighbourhoods, backlinks, join paths,
  lineage and impact analysis are recursive SQL over those links. pgvector holds embeddings for
  retrieval (phase 4).

### Deviation: the Apache AGE projection is deferred

Spec §6.2 asks for an AGE graph updated in the same transaction. Slice 3.1 doesn't build it:

- every graph question the product asks so far (a note's neighbourhood to depth 2, backlinks,
  what depends on a metric, join paths between a handful of entities) is a short recursive query
  at workspace scale (thousands of notes);
- AGE's Cypher runs inside SQL strings whose parameters and graph names can't be bound like
  ordinary SQL, so user-written titles would have to be quoted into query text — exactly the kind
  of fragile edge a reliable harness avoids;
- the gateway's tests run on plain Postgres in CI; AGE would need the custom image there too.

Because the relational tables are the source of truth, an AGE projection can be added later
without migrating data, if path queries outgrow SQL (e.g. the agent's `graph_context` tool at
much larger scale). The extension stays installed.

### Metrics in visuals (slice 3.2)

- **Entities bind per project.** A shared entity ("Order") is defined in business terms; each
  project says which of its tables it is (`entity_bindings`). A visual over metrics takes its
  table's entity as the grain; bare columns are that entity's, `customer.segment` reaches a
  related one through many-to-one relationships.
- **A visual uses metrics or raw columns, not both.** A raw aggregation has no definition to
  join or filter by, so mixing them would silently compute two different populations. The editor
  offers metrics first; raw aggregations remain, marked "Not governed".
- **A standing rule applies once approved.** A pending "always" rule (e.g. *Exclude internal
  accounts*) would otherwise change approved numbers before anyone reviewed it. Unapproved
  metrics and conditional rules can be used explicitly and carry the "Unapproved logic" badge.
- **Example cases are rows, not live numbers** (decision 6 in practice). A case gives a few rows
  per entity and the answer the definition must produce on them; the gateway compiles the
  definition as if approved and runs it on those rows in a scratch in-memory DuckDB (no files,
  no extensions). Cases test the logic, so a data refresh can never block an approval; checking
  live totals belongs to data-quality monitoring. Approval pins the version whose cases passed.
- **Impact before approval.** The reviewer sees the definitions that use the note (through
  `[[links]]` in formulas and conditions, transitively) and the visuals charting any of them;
  visuals in projects the reviewer can't open are counted, not named.
- **The result cache keys on logic.** Every definition used (`note_id@version`) and every table's
  data version are part of the key, so approving a new version produces new results.

## How knowledge gets in

1. Connecting a source: keys, joins, categorical and date columns from profiles → draft entities,
   relationships, dimensions (deterministic heuristics, no LLM needed).
2. Building sheets: a formula used repeatedly → "promote to a metric?".
3. Conversations: answers to the copilot's clarifying questions and corrections → drafts, with
   the conversation as evidence.
4. People writing notes directly.

## Project creation → dashboards

On project creation the gateway builds a **context pack**: entities matched to the project's
tables, notes retrieved by the project description, and the graph neighbourhood around them
(metrics, rules, synonyms), approved notes only, with drafts flagged. The model drafts a dashboard
plan as `VisualSpec`s over approved metrics, asks about genuine ambiguities, and the user confirms
or edits the plan before any sheet is created.

## The harness

- The model only calls typed tools and emits `VisualSpec`s; the gateway validates and compiles
  them. Unknown identifiers are rejected and the error is returned to the model for a bounded
  number of repairs.
- Before answering, the model runs a preview and checks it against profiles (empty results,
  implausible magnitudes, unit mismatches).
- A clarifying question with options beats a guess; questions that can wait queue up for the
  owner ("Questions from the agent", design screen 05).
- Definitions carry example cases that must pass before they can be published.
- Evals in CI: question → expected result, dashboard generation and knowledge capture, on the demo
  datasets; recorded model responses make CI deterministic, a nightly job runs the live model;
  prompt changes re-run the evals; the bar is ≥ 85 %.
- Every run is traced (prompt version, tools, tokens, cost, latency) and replayable, with step and
  time limits and an explained failure when the limits are hit.

## Build order

3.1 graph core (notes, links, revisions and lifecycle, owners, graph view, approval queue) · 3.2 expression language and semantic compiler, definition tests, impact on publish ·
3.3 profile-based capture, "promote to metric" · 4.1 agent harness (tool server, model loop over
the OpenAI-compatible provider, tracing, evals) · 4.2 copilot with clarifying questions · 4.3 project → dashboard generation ·
4.4 capture from conversations and the agent's question queue.
