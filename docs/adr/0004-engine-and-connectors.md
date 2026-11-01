# ADR 0004 — Query engine, extracts and connectors (phase 1)

- Status: accepted
- Date: 2026-10-31, amended 2026-11-01 at the end of phase 1
- Evidence: a feasibility probe on the reference machine (Apple silicon, 10 threads), then the
  phase 1 benchmarks ([PHASE_1_REPORT](../phases/PHASE_1_REPORT.md)).

## Engine: DuckDB 1.5.6, one database file per project

- `duckdb` crate `~1.10506.1` (DuckDB **1.5.6**, the sixth bugfix release of the 1.5 line), built
  `bundled` with `parquet` and `json`. DuckDB 2.0.0 had been out ten days at pin time; upgrade once
  it has a bugfix release and our benchmarks re-run clean.
- Each project gets `DATA_DIR/projects/<id>/project.duckdb`. **Extracts are native DuckDB tables in
  that file** (spec §2.2: "one DuckDB file per project for extracts"), not Parquet read per query.
  The uploaded original goes to object storage for durability and re-hydration.
- Arrow end to end on DuckDB's own Arrow version (arrow 58; the gateway's direct `arrow`
  dependency only adds the IPC writer, and the lockfile holds one version). Results are collected with `query_arrow` (errors surface before iteration;
  `stream_arrow` panics on mid-stream errors, which the gateway forbids).
- Queries run on blocking threads with pooled connections (`Connection::try_clone`) behind a
  per-project semaphore. Every query carries a cancellation token and a timeout (30 s default);
  either calls DuckDB's `interrupt()` (measured: the query stops ~1 ms later) and drops the result.
- The engine keeps an LRU of open projects (16 by default). A project's database is invalidated
  and reopened when a live source is added or removed, because attachments are made before the
  configuration is locked (below).

### Why native tables: 50M rows, warm

Feasibility probe (raw DuckDB):

| Aggregation over 50M rows | Parquet (zstd), per query | Native table |
|---|---|---|
| `sum(amount)` by region | 77 ms | **20 ms** |
| filtered top-10 categories | 91 ms | **32 ms** |
| filtered count | — | **21 ms** |
| sum by month (`date_trunc`) | 180 ms | 106–125 ms |
| sum by month (`year(d), month(d)`) | — | **39 ms** |
| store × region, sum + avg (3,988 groups) | 196 ms | **90 ms** |
| `approx_count_distinct(customer)` by region | — | **42 ms** |
| exact `count(DISTINCT customer)` by region (2M distinct) | 349 ms | 296 ms |

Consequences for the planner: time buckets compile to integer date parts with the bucket date
rebuilt in an outer projection over the aggregated rows (2.7× faster than `date_trunc`). Exact
distinct counts over millions of distinct values exceed the 100 ms budget on this hardware; they
stay exact by default, `approx_count_distinct` is offered, and repeats hit the Redis result cache.
The phase 1 report has the same queries measured through the engine API and end to end.

## Hardening every project database

DuckDB can read arbitrary files and URLs. After a project database is opened, its sources attached
and its extensions loaded, the engine runs
`SET allowed_directories = ['<project data dir>/']; SET enable_external_access = false;
SET lock_configuration = true;` — verified by tests: attached sources and extracts keep working,
`read_csv('/etc/passwd')` is refused, and the settings cannot be changed back. Queries are compiled
by the gateway from a structured spec with validated identifiers and bound parameters; no SQL text
from a client ever reaches DuckDB.

Live-source credentials are passed through DuckDB temporary secrets
(`CREATE TEMPORARY SECRET … ; ATTACH '' AS src (TYPE postgres, SECRET …, READ_ONLY)`), so passwords
never appear in `duckdb_databases().path`. At rest they are AES-256-GCM encrypted in Postgres with
`LATTICE_SECRET_KEY`, with the source id as associated data (a ciphertext copied onto another
source does not decrypt).

## Connectors

The spec's `Connector` trait assumes each connector streams Arrow batches itself. With DuckDB as
the federation engine, phase 1 connectors instead **register** a source with a project database
(attach, or load an extract) and let DuckDB scan it; that keeps one execution engine, one
cancellation path and one place to harden. The trait keeps the spec's shape — `kind`, `test`,
`introspect`, `profile`, `capabilities` — with `scan` replaced by DuckDB registration. ADBC
warehouses (phase 5) can implement a streaming `scan` behind the same trait if needed.

| Source | Mode | How |
|---|---|---|
| CSV / TSV, JSON / NDJSON, Parquet, Excel (each sheet a table) | Extract | uploaded, copied to object storage, loaded into native tables |
| SQLite file | Live (read-only attach) | uploaded; DuckDB `sqlite` extension |
| PostgreSQL, MySQL | Live (read-only attach) | DuckDB `postgres` / `mysql` extensions over a temporary secret |

Per-table extracts of live tables are supported by the engine (`create_table_as`) but have no UI
yet; incremental refresh is phase 5.

Extensions (`postgres`, `mysql`, `sqlite`, `excel`) are installed into a fixed directory **at image
build time** (`gateway install-extensions`), so the gateway never downloads code at runtime in a
container. Native development installs them into `DATA_DIR/duckdb-extensions` on first start.

Introspection reads `duckdb_tables()` / `duckdb_columns()` on the attached catalog (500 SQLite
tables in 1.46 s; a Postgres connect + introspect in 0.38 s). Row counts of live tables are the
server's estimates (`estimated_size`, which Postgres derives from `reltuples`); a table that was
never analysed reports −1 and is shown as unknown rather than as empty.

Profiling uses DuckDB's `SUMMARIZE` (min, max, approximate distinct, null percentage, quartiles)
plus a few sample values per column, stored on the catalog. The first 50 tables of an uploaded
source are profiled when it is added; Postgres and MySQL tables are profiled only on demand. Live
tables (including SQLite) are profiled over their first 100,000 rows, so profiling never scans a
production table end to end.

## Query path and transport

1. The browser sends `query_batch { batch_id, project_id, queries: [{ id, query: TableQuery }] }`
   on `/ws`. Each batch re-validates the session and checks the caller's project role (Viewer or
   above), so a revoked session stops at its next batch rather than holding an open data socket.
2. A newer batch on the same socket cancels the older one (spec rule 6); `cancel { batch_id }` and
   closing the socket do the same.
3. Per query: resolve the table within the project → compile → result-cache lookup → execute →
   truncate to the row limit → encode. Each result is one binary frame
   `[u32 LE header length][ResultHeader JSON][Arrow IPC stream]`; the header carries row count,
   truncation, cache hit and a timing breakdown (compile, cache, queue, execute, serialize, total).
4. **Decimals are sent as Float64.** Arrow JS (21.x) materialises Decimal128 as raw 128-bit words,
   not numbers. The gateway casts DECIMAL columns to Float64 before encoding (`ipc::for_browser`).
   Aggregation stays exact inside DuckDB; only the transport loses digits past ~15 significant
   figures, which display formatting never shows. Revisit if a consumer needs exact decimals
   (e.g. exports, which will read from DuckDB directly).

## Result cache

Redis, keyed by SHA-256 over a length-prefixed tuple of `(project, compiled SQL, bound params,
source data_version, logic_version, rls_context)`. `data_version` is bumped whenever a source
finishes a sync, so stale results are unreachable without explicit invalidation; `logic_version`
(0) and `rls_context` ("") are the seams for the business-logic layer and row-level security.
Results up to 8 MiB are cached for 10 minutes. Cache reads and writes are best-effort with a
250 ms timeout: a slow or absent Redis costs latency, never correctness.

## Building DuckDB

`bundled` compiles DuckDB's C++ amalgamation inside the gateway build. Consequences:

- A cold gateway image build takes ~10 minutes, and with default parallelism (one compiler per
  core) it ran out of memory in an 8 GB Docker Desktop VM. The Dockerfile caps compile jobs with
  `ARG BUILD_JOBS=4` (raise it on bigger builders). cargo-chef keeps the DuckDB build in a cached
  dependency layer, and CI builds through BuildKit with the GitHub Actions layer cache
  (`docker-compose.ci.yml`), so the cost is paid when dependencies change, not on every run.
- The gateway image grew from 75.6 MB to 309 MB: the binary is 70 MB (DuckDB statically linked)
  and the four extensions are 117 MB.
- Alternative not taken: link a prebuilt `libduckdb` (drop `bundled`, set `DUCKDB_LIB_DIR`). It
  removes the compile but couples the image to a separately downloaded shared library whose
  version must match the crate exactly; revisit if build time becomes a problem.

## Consequences

- One DuckDB instance per open project (buffer pool + threads each); the engine keeps an LRU of open
  projects and a per-database memory limit (`ENGINE_MEMORY_LIMIT`, 2 GB by default).
- User-supplied Postgres/MySQL hosts are an SSRF surface. Phase 1 validates the fields, refuses
  the gateway's own metadata database (sessions and password hashes live there) and probes with a
  20 s timeout. It does not yet block private or link-local addresses: an egress allow-list is a
  production requirement (phase 6), and until then only project Admins can add sources.
