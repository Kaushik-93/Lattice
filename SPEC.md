> **Note (2026-10-31).** This is the original kickoff spec, kept verbatim. The product is named
> **Lattice**; "Lumen" below was the working name, and identifiers use `lattice`
> (`lattice_ai`, `.lattice`). Decisions that amend this spec are recorded as ADRs:
> [0001 stack & boundaries](docs/adr/0001-stack.md) ·
> [0002 workspaces, roles, logic scope](docs/adr/0002-workspaces-roles-and-logic-scope.md) ·
> [0003 version pins & deviations](docs/adr/0003-version-pins-and-deviations.md).
> The UI follows [docs/design](docs/design/).

# Project Kickoff Prompt — "Lumen" (working name): A GenAI-Native, Ultra-Fast BI Web Platform

> Paste this whole document into your coding agent as the opening prompt, or commit it as `SPEC.md` / `CLAUDE.md` at the repo root.

---

## 0. Your role

You are the founding engineer of a GenAI-native business-intelligence **web application**, comparable in scope to Power BI, built around natural-language questions and a governed, human-approved business-logic layer. You write production-quality **TypeScript** (frontend), **Rust** (data gateway) and **Python** (AI service).

**Performance is the product.** Every design decision must be justified against the latency budgets in §3. When a choice trades speed for convenience, choose speed and say so in a code comment.

Work in phases (§12). At the end of each phase everything builds, tests pass, benchmarks run, `docker compose up` works from a clean clone, and you write `docs/phases/PHASE_N_REPORT.md` with what was built, benchmark numbers and open issues. Do not start the next phase until the current one meets its exit criteria.

---

## 1. Product summary

Users connect any number of data sources, ask questions in plain language, and get correct, interactive charts instantly. Between the raw data and the AI sits a **Business Logic Layer (BLL)**: a semantic model of entities, relationships, metrics, dimensions and business rules, stored relationally and projected into a **graph** for dependency, lineage and context retrieval.

The AI never invents business meaning silently. When it needs a new definition, assumption or rule, it **proposes** it into the BLL as a pending item. A human approves, edits or rejects it. Only approved logic is applied to queries.

Work is organised into **Projects**. Each project has its own connections, its own business logic, and multiple **Sheets** (report pages). Each sheet holds a grid of **Visuals** (charts, tables, KPI cards, filters).

---

## 2. Tech stack (decided — deviations require an ADR in `docs/adr/`)

### 2.1 Frontend — `apps/web`
| Concern | Choice |
|---|---|
| Framework | **Next.js (App Router) + React + TypeScript (strict)** |
| Styling / components | Tailwind CSS + shadcn/ui (Radix primitives) |
| Client state | **Zustand** with fine-grained selectors (large data never lives in React state) |
| Server state (small control calls) | TanStack Query |
| Data transport | **WebSocket + Arrow IPC** straight to the Rust gateway; decoded with `apache-arrow` JS (no JSON for data) |
| Charts | **Apache ECharts** (canvas renderer) for standard visuals; **deck.gl / regl (WebGL)** for scatter and series beyond ~200k points |
| Tables | TanStack Table + TanStack Virtual (windowed fetch from the gateway) |
| Sheet layout | `react-grid-layout` (drag, resize, snap) |
| Editors | Monaco (SQL, YAML for business logic) |
| Client compute | Rust → **WASM** package (`packages/wasm`) for local filtering, binning and LTTB downsampling, run in a Web Worker |
| Testing | Vitest, React Testing Library, Playwright (E2E + interaction-latency tests) |

**Next.js rules:**
- Use Next.js for the app shell, routing, auth pages, settings and project lists.
- The sheet canvas and every visual are **client components** (`"use client"`). Server rendering does not help the canvas.
- **Never proxy data through Next.js API routes or server actions.** The browser talks to the Rust gateway directly (WebSocket for data/streams, REST for small control calls). Next.js route handlers are allowed only for things like OG images or health checks.
- Arrow tables are held in refs or a non-reactive store; components subscribe to derived, minimal slices.
- Memoise visuals (`React.memo`) and key them by spec hash, so a cross-filter re-renders only the visuals whose data changed.

### 2.2 Data gateway — `services/gateway` (Rust)
| Concern | Choice |
|---|---|
| Language | Rust stable, 2024 edition, Cargo workspace |
| HTTP / WS | **Axum** + `tokio` + `tower` (compression, timeouts, rate limits) |
| Query engine | **DuckDB** (embedded, `duckdb` crate), one DuckDB file per project for extracts; DataFusion kept behind an `Engine` trait as an alternative |
| In-memory format | **Apache Arrow** (`arrow-rs`) end to end |
| Connectors | DuckDB extensions (Postgres, MySQL, SQLite, Parquet, CSV, Excel, JSON, S3/GCS/Azure, Iceberg, Delta) + **ADBC** drivers (Snowflake, BigQuery, Redshift, SQL Server, Databricks) + a Rust `Connector` trait for REST/SaaS |
| Metadata DB access | `sqlx` (compile-time checked queries) against Postgres |
| Graph access | Cypher via Apache AGE through `sqlx` (`SELECT * FROM cypher(...)`) |
| Service-to-service | **gRPC** (`tonic`) to the AI service; **Arrow Flight** (`arrow-flight`) for any data handed to Python |
| Auth | Gateway owns auth: OIDC login (`openidconnect`) + optional email/password (argon2); httpOnly secure session cookie; RBAC checked on every call |
| Secrets | Connection credentials encrypted at rest (AES-GCM, key from env / KMS); never logged |
| Observability | `tracing` + OpenTelemetry export; per-request timing breakdown returned to the UI |
| Testing | `cargo nextest`, `criterion` benchmarks, `insta` snapshot tests |

### 2.3 AI service — `services/ai` (Python)
| Concern | Choice |
|---|---|
| Language / tooling | Python 3.12+, **uv** for packaging, `ruff`, `mypy --strict`, `pytest` |
| Interface | **gRPC server** (`grpcio`, async) called only by the gateway; small FastAPI app for health/metrics |
| LLM | `LlmProvider` protocol; default **Anthropic Claude** via the official SDK with tool use and structured output; optional OpenAI-compatible provider for self-hosted models |
| Agent loop | Thin, hand-written tool loop (no heavy agent framework in v1); prompts in versioned files under `services/ai/prompts/` |
| Validation | Pydantic v2 models mirroring the protobuf contracts |
| Retrieval | Embeddings stored in Postgres via **pgvector**; graph neighbourhood expansion via the gateway's graph tools |
| Data analysis (later phases) | Polars + PyArrow (fed by Arrow Flight), statsforecast / scikit-learn for forecasting and anomaly detection |

**Hard rule:** the AI service **never connects to any data source or database directly**. Everything goes through gateway tools over gRPC, so approvals, row-level security and auditing are enforced in one place.

### 2.4 Storage and infrastructure
| Concern | Choice |
|---|---|
| Metadata + business logic + audit | **PostgreSQL 18** |
| Graph | **Apache AGE** extension in the same Postgres (openCypher) |
| Vectors | **pgvector** extension in the same Postgres |
| Extracts / result cache | Per-project DuckDB files + Parquet on a volume (S3-compatible storage in production; **MinIO** in local dev) |
| Hot cache, pub/sub, job queue | **Redis** (Valkey also acceptable) |
| Local orchestration | **Docker Compose** |
| Production | Container images per service; Kubernetes/Helm later (phase 6) |

---

## 3. Performance budgets (non-negotiable; enforced by benchmarks and Playwright timing tests in CI)

| Interaction | Budget (p95) |
|---|---|
| First load of a sheet (warm cache, broadband) | < 1.2 s to interactive; skeletons within 100 ms |
| Switching sheets | < 100 ms (prefetched) |
| Cross-filter / slicer click → all visuals on sheet updated (extract data, ≤ 50M rows) | < 150 ms including network on the same region |
| Pan / zoom / hover on a chart | 60 fps; no main-thread task > 50 ms (target ≤ 16 ms) |
| Gateway overhead on a cached query | < 5 ms |
| NL question → first chart, our own overhead (excluding LLM time) | < 200 ms |
| NL question → first chart end to end | < 3 s typical, with streamed progress from 0 ms |
| Schema introspection of a 500-table source | < 5 s, cached afterwards |

Rules that follow:
1. **Never block the UI thread.** Heavy client work runs in a Web Worker (WASM).
2. **Push aggregation down.** Visuals request aggregated results; raw rows only for paginated, virtualised tables.
3. **Cap points per series** sent to the browser (default 100k); downsample with LTTB/M4 in the gateway.
4. **Cache in layers:** Redis (hot results, keyed on compiled-SQL hash + data version + BLL version + RLS context) → DuckDB extract → source.
5. **Pre-warm:** when a sheet opens, prefetch neighbouring sheets' queries in the background.
6. **Cancel stale work:** every query carries a cancellation token; a newer filter state cancels older in-flight queries end to end (browser → gateway → engine).
7. **Batch per sheet:** one WebSocket message carries all visual queries of a sheet; the gateway runs them in parallel and streams results back as each completes.
8. **Measure everything:** each response includes timings (compile, queue, execute, serialise, transfer); a dev overlay (`Ctrl+Shift+P`) shows them alongside render time.

---

## 4. Architecture

```
Browser ─ Next.js + React + TS ─────────────────────────────────────────────────────────
  Project explorer │ Sheet canvas (grid of visuals) │ Ask bar │ Business Logic panel
  Web Worker (WASM: filter, bin, downsample)
        │ WebSocket (Arrow IPC + JSON control)          │ REST (small control calls)
        ▼                                               ▼
Rust gateway (Axum) ───────────────────────────────────────────────────────────────────
  api        – REST + WS handlers, sessions, RBAC
  project    – projects, sheets, visuals, layouts
  connectors – Connector trait, DuckDB ext + ADBC + REST, introspection, profiling
  catalog    – unified schema catalog, column stats, sample values
  semantic   – Business Logic Layer: model, versions, approvals, compiler → SQL
  graph      – AGE projection of the BLL: dependencies, lineage, impact analysis
  planner    – VisualSpec → semantic query → SQL, chart-type defaults
  engine     – DuckDB/DataFusion execution, cancellation, downsampling
  cache      – Redis result cache, extract manager, refresh scheduler
  security   – credential encryption, row-level security, audit log
  aibridge   – gRPC client to AI service + gRPC tool server the AI calls back into
        │ gRPC (+ Arrow Flight)                     │ sqlx
        ▼                                           ▼
Python AI service                           PostgreSQL 18 (+ AGE graph, + pgvector)
  LLM tool loop, proposals,                 Redis · MinIO/S3 (Parquet, DuckDB files)
  embeddings, (later) forecasting
```

---

## 5. Repository layout

```
/
├─ apps/web/                     # Next.js app
├─ services/gateway/             # Rust Cargo workspace
│   ├─ crates/{api,project,connectors,catalog,semantic,graph,planner,engine,cache,security,aibridge,telemetry}
│   └─ bin/gateway/
├─ services/ai/                  # Python (uv) project
│   ├─ src/lumen_ai/{server,agent,providers,tools,retrieval,prompts}
│   └─ tests/
├─ packages/proto/               # Protobuf contracts (gateway ⇄ AI); codegen for Rust and Python
├─ packages/wasm/                # Rust → WASM for the browser
├─ packages/shared-types/        # TS types generated from Rust (ts-rs) for REST/WS messages
├─ infra/docker/                 # Dockerfiles, Postgres init scripts
├─ docker-compose.yml
├─ evals/                        # NL question → expected result datasets and runner
├─ docs/adr/  docs/phases/
└─ Makefile / justfile           # one-command dev, test, bench, eval
```

Contracts are generated, never hand-copied: protobuf → Rust (`tonic-build`) and Python (`grpcio-tools`); Rust message types → TypeScript (`ts-rs`).

---

## 6. The Business Logic Layer (the heart of the product)

### 6.1 Contents
- **Entities** (Customer, Order) mapped to physical tables across any connector.
- **Relationships** (joins with cardinality and direction), including cross-source joins.
- **Dimensions** (columns or expressions, types, hierarchies such as Year → Quarter → Month, formats).
- **Measures / metrics** (e.g. `Net Revenue = SUM(order_lines.amount) - SUM(refunds.amount)`) with default aggregation, format and allowed dimensions.
- **Business rules** ("fiscal year starts 1 April", "exclude test accounts", "Active Customer = ordered in the last 90 days").
- **Synonyms and glossary** ("sales", "turnover" → `Net Revenue`).
- **Row-level security rules.**

### 6.2 Storage model
- **Relational tables in Postgres are the source of truth**: every object has `id`, `project_id`, `kind`, `name`, `description`, `definition` (JSONB), `status` (`draft | pending_approval | approved | rejected | deprecated`), `created_by` (`user:<id>` or `ai`), `approved_by`, `version`, `rationale`, `evidence` (sample query + result), timestamps. Full version history in a separate `_versions` table.
- **YAML export/import** of the whole BLL per project so it can be diffed and kept in git.
- **Graph projection in Apache AGE** (one graph per project), **updated in the same transaction** as the relational change:
  - Nodes: `Source`, `Table`, `Column`, `Entity`, `Dimension`, `Measure`, `Rule`, `Term`, `Visual`, `Sheet`, `Proposal`.
  - Edges: `MAPS_TO`, `HAS_COLUMN`, `JOINS` (with keys and cardinality), `DEPENDS_ON`, `FILTERED_BY`, `SYNONYM_OF`, `USED_BY`, `PROPOSES`, `SUPERSEDES`.
  - Used for: **impact analysis** (what breaks or must be re-run when X changes), **lineage views** (visual → measure → column → source), **join-path finding** between entities, and **AI context retrieval** (expand the neighbourhood of matched terms).
- **Embeddings** of names, descriptions and synonyms in pgvector for semantic search.

### 6.3 Approval workflow
1. When answering a question, the AI may need logic that doesn't exist yet (an undefined metric, an ambiguous join, a missing date rule). It **must not** silently treat it as truth. It calls `propose_logic`, creating a **Proposal** with a plain-English explanation, the exact definition, the compiled SQL and a preview result.
2. The AI may answer the current question **provisionally**, clearly badged "Uses unapproved logic" with a link to the proposal. Provisional visuals cannot be pinned to a sheet until the logic is approved.
3. The **Business Logic panel** lists pending proposals with Approve / Edit-then-approve / Reject (reason required). It shows a diff, the impact analysis from the graph, and a preview. Rejection reasons are stored and fed back to the AI as context.
4. On approval (single transaction): bump the object version, update the AGE projection, invalidate affected cache keys, write the audit log, and publish an event on Redis. The gateway re-runs affected visuals and pushes updates to connected clients over WebSocket.
5. Roles: `viewer`, `analyst` (can ask and propose), `steward` (can approve), `admin`. Approval is required even when one person holds all roles.
6. Every change is diffable and revertible (per-object history plus project snapshots).

### 6.4 Bootstrapping
On first connection: introspect schemas, profile columns (cardinality, nulls, min/max, samples), detect likely keys and foreign keys, then have the AI draft an initial semantic model. **Everything arrives as pending proposals**, grouped so the user can bulk-approve sensible items in a few clicks.

---

## 7. AI question-answering pipeline

The LLM never writes raw SQL against physical tables. It works against the semantic model.

1. **Gateway → AI (gRPC `Ask`)** with project id, question, sheet context, current filters, recent turns and the user's role. Responses stream back (`stream AskEvent`) so the UI shows progress immediately.
2. **Context assembly** in Python (< 50 ms): pgvector + keyword search over BLL objects, then a graph neighbourhood expansion via the gateway's `graph_context` tool.
3. **Tool loop.** Tools the model can call (all implemented in the gateway, exposed over gRPC):
   - `search_semantic_model(query)`
   - `graph_context(object_ids, depth)` (related entities, join paths, dependent measures)
   - `get_object(id)`
   - `profile_column(entity, column)` (cached stats and sample values)
   - `run_semantic_query(spec)` (small preview so the model can self-check)
   - `propose_logic(object)` (creates a pending proposal)
   - `ask_clarifying_question(text, options[])`
   - `emit_visual(visual_spec)` (final answer)
4. **Output contract**: a strictly validated `VisualSpec` (protobuf + Pydantic + Rust validation):
   ```json
   {
     "title": "Net revenue by month, 2026",
     "chart": "line",
     "measures": ["net_revenue"],
     "dimensions": ["order_date.month"],
     "filters": [{"dim": "order_date.year", "op": "=", "value": 2026}],
     "sort": [{"by": "order_date.month", "dir": "asc"}],
     "limit": null,
     "uses_pending_logic": [],
     "explanation": "Sums approved Net Revenue per calendar month…"
   }
   ```
5. **Deterministic compilation** in Rust (`semantic` + `planner`): VisualSpec → semantic query → SQL (DuckDB dialect for extracts, source dialect for live pushdown). Unknown identifiers are rejected before execution. RLS predicates are injected here, never by the AI.
6. **Execute → Arrow → browser**, streamed over the WebSocket.
7. **Explainability**: every AI visual has a "How was this calculated?" drawer showing measures, rules, approval status, the lineage path from the graph, and the generated SQL.
8. **Chart selection**: rule-based defaults (time → line; few categories → bar; part-to-whole → stacked bar; two measures → scatter; single value → KPI) which the model may override with a stated reason.
9. **Evaluation harness** in `evals/`: question → expected-result pairs on demo datasets (TPC-H plus a retail demo). Runs in CI; accuracy is tracked per commit.

---

## 8. Projects, sheets, visuals

- **Project**: name, members and roles, connections, BLL, extracts, refresh schedule.
- **Sheet**: ordered tabs, responsive grid layout, sheet-level filters, theme.
- **Visual types (v1)**: KPI card, line, area, bar/column (grouped/stacked), scatter, pie/donut, table (virtual scroll), pivot/matrix, heatmap, histogram, slicers. Maps in phase 5.
- **Interactions**: cross-filtering, drill-down on hierarchies, tooltips, brush-to-zoom, export (PNG, CSV, Parquet, XLSX).
- **Undo/redo** for layout and spec edits (command pattern, persisted per session).
- **Autosave**; project export/import as a `.lumen` archive (YAML BLL + layouts, optional Parquet extracts).
- **Real-time presence** (phase 6): see who is viewing or editing a sheet; changes broadcast over WebSocket.

---

## 9. Connectors

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    fn kind(&self) -> ConnectorKind;
    async fn test(&self) -> Result<()>;
    async fn introspect(&self) -> Result<SchemaCatalog>;
    async fn profile(&self, table: &TableRef, opts: ProfileOpts) -> Result<TableProfile>;
    async fn scan(&self, query: PushdownQuery, cancel: CancellationToken)
        -> Result<SendableRecordBatchStream>; // Arrow batches
    fn capabilities(&self) -> Capabilities;     // pushdown support, dialect, limits
}
```
- **v1**: CSV/Parquet/Excel/JSON upload, PostgreSQL, MySQL, SQL Server, SQLite, Snowflake, BigQuery, S3/Azure Blob/GCS.
- **v2**: Databricks, Redshift, Delta/Iceberg, generic REST/OpenAPI, Google Sheets, Salesforce.
- **Modes per table**: *Live* (push down to the source) or *Extract* (Parquet/DuckDB with incremental refresh on a watermark column). Default to Extract under a configurable size.
- Uploaded files go to MinIO/S3 and are registered as Parquet.
- Large warehouses: prefer Live mode with aggregation pushdown rather than copying data into the gateway.

---

## 10. Local development: Docker Compose

Create `docker-compose.yml` with these services and health checks; `docker compose up` must give a working app at `http://localhost:3000` with a seeded demo project.

```yaml
services:
  web:        # Next.js (dev: next dev with hot reload; prod: standalone output)
    build: ./apps/web
    ports: ["3000:3000"]
    environment:
      NEXT_PUBLIC_GATEWAY_URL: http://localhost:8080
    depends_on: [gateway]

  gateway:    # Rust Axum
    build: ./services/gateway
    ports: ["8080:8080"]
    environment:
      DATABASE_URL: postgres://lumen:lumen@postgres:5432/lumen
      REDIS_URL: redis://redis:6379
      AI_GRPC_URL: http://ai:50051
      S3_ENDPOINT: http://minio:9000
      DATA_DIR: /data
    volumes: [gateway_data:/data]
    depends_on: [postgres, redis, minio]

  ai:         # Python gRPC
    build: ./services/ai
    environment:
      GATEWAY_TOOLS_URL: http://gateway:50052
      ANTHROPIC_API_KEY: ${ANTHROPIC_API_KEY}
      DATABASE_URL: postgres://lumen:lumen@postgres:5432/lumen  # pgvector reads only, via a read-only role
    depends_on: [gateway]

  postgres:   # PostgreSQL 18 + Apache AGE + pgvector
    build: ./infra/docker/postgres
    environment:
      POSTGRES_USER: lumen
      POSTGRES_PASSWORD: lumen
      POSTGRES_DB: lumen
    command: ["postgres", "-c", "shared_preload_libraries=age"]
    ports: ["5432:5432"]
    volumes:
      - pg_data:/var/lib/postgresql
      - ./infra/docker/postgres/init:/docker-entrypoint-initdb.d

  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]

  minio:
    image: minio/minio
    command: server /data --console-address ":9001"
    ports: ["9000:9000", "9001:9001"]
    volumes: [minio_data:/data]

volumes: { pg_data: {}, gateway_data: {}, minio_data: {} }
```

`infra/docker/postgres/Dockerfile`:
```dockerfile
FROM postgres:18
RUN apt-get update \
 && apt-get install -y --no-install-recommends postgresql-18-age postgresql-18-pgvector \
 && rm -rf /var/lib/apt/lists/*
```

`infra/docker/postgres/init/01-extensions.sql`:
```sql
CREATE EXTENSION IF NOT EXISTS age;
CREATE EXTENSION IF NOT EXISTS vector;
LOAD 'age';
```

Notes for the agent:
- Pin exact image tags and package versions once verified; record them in an ADR.
- The AI service's database role is **read-only and limited to the embeddings table**. All other access goes through gateway tools. (If this proves awkward, move embedding search behind a gateway tool and remove the AI service's DB access entirely.)
- Every AGE session must run `LOAD 'age'; SET search_path = ag_catalog, "$user", public;` — do this in the `sqlx` connection `after_connect` hook.
- Multi-stage Dockerfiles: Rust with `cargo-chef` for cached builds; Python with `uv sync --frozen`; Next.js with `output: "standalone"`.
- A `make dev` (or `just dev`) target starts everything with hot reload; `make seed` loads the demo dataset and a pre-approved demo BLL.

---

## 11. Engineering rules

- **Rust**: `#![forbid(unsafe_code)]` except isolated FFI; `thiserror` in libraries, `anyhow` only in binaries; no `unwrap()` outside tests; `clippy -D warnings`.
- **TypeScript**: `strict: true`, no `any`, ESLint + Prettier; generated types only for gateway messages.
- **Python**: `mypy --strict`, `ruff`, Pydantic models at every boundary.
- Benchmarks (`criterion`) for every hot path in the gateway; Playwright timing tests for the budgets in §3; CI fails on > 10% regression.
- Snapshot-test the semantic compiler (VisualSpec → SQL) with `insta`.
- LLM prompts live in versioned files, never inline strings.
- Every significant decision or deviation gets an ADR.
- Security: secrets only via env/secret store; parameterised SQL everywhere; RLS applied in the compiler; audit log for every BLL change and every query run on behalf of the AI.

---

## 12. Phased delivery plan

**Phase 0 — Skeleton** (exit: `docker compose up` works from clean clone; CI green)
Monorepo, Compose stack, Postgres with AGE + pgvector, Next.js shell with auth pages, Axum gateway with health checks and OIDC/email login, Python gRPC stub, protobuf codegen, tracing, perf overlay, CI (lint, type-check, tests, benchmarks).

**Phase 1 — Engine and connectors** (exit: 50M-row Parquet aggregations < 100 ms warm in the gateway)
DuckDB engine, Arrow over WebSocket to the browser, CSV/Parquet/Excel upload, Postgres/MySQL/SQLite connectors, introspection and profiling, Redis result cache, end-to-end cancellation.

**Phase 2 — Visuals and sheets** (exit: 60 fps pan/zoom on 1M points; cross-filter < 150 ms)
ECharts + WebGL renderers, WASM worker, core chart types, grid layout, project/sheet/visual persistence, cross-filtering, drill-down, undo/redo.

**Phase 3 — Business Logic Layer + graph** (exit: propose → approve → apply → re-run cycle works, with impact analysis and audit log)
Semantic model tables and versioning, YAML import/export, AGE projection kept in sync transactionally, compiler to SQL, approval UI with diff and impact view, lineage view, auto-discovery bootstrap.

**Phase 4 — AI** (exit: ≥ 85% accuracy on the eval set; no visual ever uses unapproved logic without a visible badge)
Python tool loop, Claude provider, gateway tool server, pgvector retrieval + graph context, VisualSpec validation, streaming ask UX, clarifying questions, explainability drawer, eval harness in CI.

**Phase 5 — Scale and polish**
Snowflake/BigQuery/SQL Server/S3 connectors, live pushdown mode, incremental extract refresh and schedules, row-level security, exports, maps, forecasting and anomaly detection in the AI service.

**Phase 6 — Production and collaboration**
Helm charts, S3 storage, horizontal scaling of the gateway (sticky WebSocket sessions or Redis pub/sub fan-out), real-time presence, SSO hardening, usage metering.

---

## 13. Start now

Begin with **Phase 0**. First output:
1. The repository tree you intend to create.
2. The Compose file, Postgres Dockerfile and init scripts, with verified version pins.
3. `docs/adr/0001-stack.md` restating the stack, the service boundaries (especially "AI never touches data directly") and the performance budgets.
4. Then implement Phase 0 fully and report: clean-clone `docker compose up` time, gateway health latency and the Next.js first-load timing.

Ask me questions only if something in this spec is contradictory. Otherwise make the reasonable call, note it in the phase report, and keep going.
