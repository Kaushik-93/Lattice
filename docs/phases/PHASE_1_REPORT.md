# Phase 1 report — Engine & connectors

- Date: 2026-11-01
- Exit criterion: 50M-row aggregations < 100 ms warm in the gateway ✅ for every query shape
  except an exact distinct count over 2M distinct values (340 ms; see open issue 1). Also:
  `docker compose up` from a clean clone ✅ (all 12 Playwright tests pass against it) · CI green:
  the run for the push of this phase is linked at the end of this report.

Measured on the same machine as phase 0: Apple-silicon Mac (10 cores, 16 GB), Docker Desktop with
an 8 GB VM.

**Scope change during the phase:** the product owner decided that the business-logic layer
(phase 3) and the AI copilot (phase 4) will be built interactively with them. Phase 2 (visuals and
sheets) comes next, then the non-AI, non-logic parts of phases 5–6. The seams phase 3/4 need are in
place: the result-cache key carries `logic_version` and `rls_context`, and the AI service stays a
stub.

## What was built

**Engine** (`crates/engine`) — DuckDB 1.5.6, one database file per project, an LRU of open
projects, pooled connections behind a semaphore, per-query cancellation and timeouts (both call
DuckDB's `interrupt()`), and a lockdown applied after sources are attached: only the project's own
directory is readable, external access is off, and the configuration is locked. Loads CSV/TSV,
JSON/NDJSON, Parquet and Excel (one table per sheet) into native tables; attaches SQLite, Postgres
and MySQL read-only; introspects; profiles with `SUMMARIZE`. Results leave as Arrow IPC.
[ADR 0004](../adr/0004-engine-and-connectors.md) has the design and its reasons.

**Planner** (`crates/planner`) — `TableQuery` (dimensions with day/week/month/quarter/year buckets,
sum/avg/min/max/count/count distinct/approximate distinct, typed filters, sort, limit/offset) →
DuckDB SQL with quoted identifiers and bound parameters. Snapshot tests pin the generated SQL.

**Connectors and catalog** (`crates/connectors`, `crates/catalog`) — source configs and
validation; attach statements that pass credentials through DuckDB temporary secrets; sources,
tables, columns and profiles in Postgres; a per-source `data_version` bumped on each sync.
Passwords are AES-256-GCM encrypted at rest (`LATTICE_SECRET_KEY`, source id as associated data).

**Result cache** (`crates/cache`) — Redis, keyed by SHA-256 over project, SQL, parameters,
`data_version`, `logic_version` and `rls_context`; 10 minute TTL, 8 MiB cap, best effort with a
250 ms timeout.

**API and data socket** (`crates/api`)
- `GET/POST /projects/{id}/sources`, `POST /projects/{id}/sources/upload` (multipart, streamed to
  disk, 200 MB limit, original copied to object storage), `GET/DELETE …/sources/{source}`,
  `POST …/refresh`, `POST /projects/{id}/tables/{table}/profile`. Viewers read, Editors profile,
  Admins add, refresh and delete.
- `/ws` runs query batches. Every batch re-validates the session and the caller's role (phase 0
  open issue 1), a newer batch cancels the older one, and each result is one binary frame: a JSON
  header (rows, truncation, cache hit, timing breakdown) followed by an Arrow IPC stream.
- Demo data: "Revenue Analytics" gets a generated retail dataset of 1M orders on a fresh seed
  (phase 0 open issue 11); `gateway seed-bench N` adds a "Benchmark · N orders" project.

**Web** — screen 04 of the design (project sources: list, upload with progress, add a database,
refresh, delete), a source page with table and column profiles, and a data explorer that groups,
buckets, aggregates, filters and sorts any table over the socket into a virtualised grid with the
gateway's timings. Arrow results live in a non-reactive `ResultStore` read through
`useSyncExternalStore`, outside React state. The project overview's Sources panel is live, and the
perf overlay lists socket queries (client time beside the gateway's breakdown).

**Infrastructure** — DuckDB extensions baked into the gateway image; an `s3-init` step creates the
bucket before the gateway starts; an `acme_ops` demo Postgres database with a read-only user for
trying live sources; CI gains Redis for the gateway tests, BuildKit layer caching for images
(phase 0 open issue 7) and a 50M-row end-to-end benchmark in the stack job.

## Numbers

### 50M-row aggregations through the engine (criterion)

`crates/engine/benches/aggregate.rs`: planner-compiled SQL through the engine API (semaphore,
pooled connection, blocking thread, DuckDB, Arrow materialisation), warm, on the generated orders
table (50M rows, 2.5M customers, 60 stores). Native build on the host.

| Query | Mean | 95% CI |
|---|---|---|
| `sum(amount)` by region | **22.1 ms** | 21.7–22.4 |
| sum + count by month | **38.8 ms** | 38.4–39.4 |
| top 10 categories since 2025-01-01 | **28.6 ms** | 28.1–29.1 |
| sum + avg by store × region | **64.7 ms** | 64.2–65.4 |
| count + sum, two equality filters | **27.0 ms** | 26.7–27.3 |
| approximate distinct customers by region | **30.7 ms** | 30.5–30.9 |
| exact distinct customers by region | 340 ms | 335–346 |
| Arrow IPC encode of the monthly result (incl. decimal → float) | 1.6 µs | |

### End to end over the data socket, 50M rows (Docker)

`scripts/bench-ws.mjs` against the Compose stack: batch sent → Arrow frame received and decoded in
Node, through Docker Desktop's port forwarding. Median of 7; "uncached" flushes Redis before every
run.

| Query | Uncached, client | Uncached, inside the gateway | Cached, client |
|---|---|---|---|
| `sum(amount)` by region | 23.7 ms | 22.5 ms | 0.93 ms |
| sum + count by month | 31.4 ms | 30.3 ms | 0.79 ms |
| top 10 categories, filtered | 25.3 ms | 24.2 ms | 0.77 ms |
| sum + avg by store × region | 62.8 ms | 61.6 ms | 0.78 ms |
| approximate distinct by region | 75.7 ms | 74.3 ms | 0.74 ms |
| exact distinct by region | 324.6 ms | 323.4 ms | 0.89 ms |

Gateway overhead around DuckDB (compile, cache lookup, framing, socket) is about 1 ms; a cache hit
is under 1 ms end to end, against the "< 5 ms gateway overhead on a cached query" budget.

### In the browser

| Interaction (Playwright, production build) | Median | Budget |
|---|---|---|
| Regroup the 1M-row demo table in the explorer, click → grid re-rendered | 34.4 ms (4.3 ms in the gateway) | < 150 ms |

### Connectors, seeding and builds

| | |
|---|---|
| Introspect a 500-table SQLite file | 1.46 s |
| Connect to Postgres + introspect (`acme_ops`) | 0.38 s |
| Cancel a running query (`interrupt()` → query stops) | ~1 ms |
| Generate the 1M-order demo dataset on first start (Compose) | 0.58 s |
| `gateway seed-bench 50000000` (Compose) | 23.7 s |
| Clean clone (323 files, 7.3 MB) → `docker compose build --no-cache` | 493 s (DuckDB's C++ dominates) |
| `up --wait` until all services are healthy, demo dataset included | 18 s |

Images: gateway 309 MB (70 MB binary with DuckDB linked in, 117 MB of extensions; 75.6 MB in
phase 0), ai 258 MB, web 439 MB, postgres 672 MB.

### Tests

| Suite | Count | Result |
|---|---|---|
| Rust: unit, planner snapshots, engine (lockdown, cancellation, timeouts, Excel, SQLite, unreachable hosts), API integration on real Postgres + Redis | 129 | pass |
| Python | 10 | pass |
| Web unit (frame parsing, query building, components) | 28 | pass |
| Playwright e2e + latency budgets (demo dataset, CSV upload → query, live Postgres, refused credentials, regroup budget, phase 0 suites) | 12 | pass |

## Decisions made during the phase

Made without asking ([ADR 0004](../adr/0004-engine-and-connectors.md) has the details):
1. **Extracts are native DuckDB tables**, not Parquet read per query: 2–4× faster on every
   aggregation measured. The spec's exit criterion names Parquet; the native-table numbers are
   the ones above, the Parquet ones are in the ADR.
2. **Connectors register sources with DuckDB** instead of streaming Arrow themselves: one engine,
   one cancellation path, one place to harden.
3. **Time buckets compile to integer date parts** (2.7× faster than `date_trunc` at 50M rows).
4. **Exact distinct counts stay exact**; approximate distinct is a separate aggregation.
5. **Decimals travel as Float64**, because Arrow JS doesn't turn Decimal128 into numbers.
6. **Postgres and MySQL sources are live (read-only attach)** by default; per-table extracts exist
   in the engine without a UI yet.
7. **Live tables are profiled over their first 100,000 rows**, and Postgres/MySQL only on demand.
8. **Source management is Admin-only**, profiling is Editor+, reading is Viewer+.
9. **DuckDB's C++ build is capped at 4 parallel jobs** in Docker (it ran out of memory in an 8 GB
   VM otherwise); extensions are installed at image build time, never downloaded at runtime.

## Open issues and follow-ups

1. **Exact distinct over millions of values is ~340 ms** at 50M rows, over the 100 ms budget.
   Approximate distinct (31 ms) is available and repeats hit the cache; a pre-aggregated distinct
   sketch per extract is the fix if users need exact counts interactively.
2. **SSRF:** a user-supplied database host may be any address except the metadata database.
   Production needs an egress allow-list (phase 6); until then only project Admins add sources.
3. **MySQL is untested against a real server**: the attach SQL is unit-tested, but Compose has no
   MySQL. Add one to the test stack when MySQL users appear, or in phase 5 with the warehouses.
4. **Uploads load synchronously** inside the request (10 minute timeout on the upload route). A
   background job with progress events belongs with incremental refresh (phase 5).
5. **Originals are copied to object storage but never restored**: losing the gateway's data
   volume loses extracts until they are re-uploaded. Add re-hydration from object storage.
6. **Memory:** each open project database may use up to `ENGINE_MEMORY_LIMIT` (2 GB), and up to 16
   can be open. A global budget is needed before many large projects share one gateway.
7. **No key rotation for `LATTICE_SECRET_KEY`**; ciphertexts would need a key id to rotate.
8. A cold gateway image build takes ~10 minutes (DuckDB's C++). CI caches the layer; linking a
   prebuilt `libduckdb` is the alternative if that stops being enough.
9. The CI benchmark job now runs the 50M-row engine bench on shared runners, where absolute
   numbers differ from the table above; the regression gate compares against the target branch on
   the same runner, and only fails when the whole confidence interval is past 10%.
10. Phase 0 issues 2–6, 8–10 and 12 still stand.

## Next: phase 2 (visuals & sheets)

ECharts (WebGL for large series), rendering off the main thread, the chart types, a sheet grid
layout, sheet and visual persistence, cross-filtering, drill-down and undo/redo, with the
cross-filter and sheet-switch budgets enforced by Playwright. Phases 3 and 4 wait for the
product owner.
