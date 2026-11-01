# Lattice

**Ask your data. Keep the logic.** A GenAI-native business-intelligence platform: connect data
sources, ask questions in plain language, get interactive charts — built on a governed,
human-approved business-logic layer, so every number is traceable to a definition someone signed
off.

Status: **phase 1 (engine & connectors)** — on top of the phase 0 skeleton (auth, workspaces,
projects, app shell, gateway, AI service stub, contracts, CI): a DuckDB query engine per project,
file uploads (CSV, JSON, Parquet, Excel, SQLite) and live Postgres/MySQL sources, introspection and
profiling, a data explorer streaming Arrow results over WebSocket, a Redis result cache and
end-to-end cancellation. See [docs/phases/PHASE_1_REPORT.md](docs/phases/PHASE_1_REPORT.md).

## Run it

Requires Docker.

```sh
docker compose up --build        # or: make up
```

Open http://localhost:3000 and sign in as **demo@lattice.dev / lattice-demo** (seeded "Acme Retail"
workspace), or create your own workspace. Press **Ctrl+Shift+P** for the performance overlay.

The "Revenue Analytics" project comes with a generated retail dataset (1M orders) under
**Sources**; open a table to group, filter and aggregate it. To add a live Postgres source, the
stack ships a demo database: host `postgres`, port `5432`, database `acme_ops`, user `acme_reader`,
password `acme-reader`. `make seed-bench` adds a 50M-row project for benchmarking.

| Service | URL |
|---|---|
| Web app | http://localhost:3000 |
| Gateway (REST, WebSocket `/ws`) | http://localhost:8080 — `/healthz`, `/readyz` |
| Object storage console (RustFS) | http://localhost:9001 (lattice / lattice-dev-secret) |
| Postgres (AGE + pgvector) | localhost:5433 (lattice / lattice) |

Try single sign-on locally with the bundled dev identity provider: copy the `OIDC_SSO_*` lines from
[.env.example](.env.example) into `.env`, then `docker compose --profile sso up`.

## Develop

Needs Rust (rustup), [uv](https://docs.astral.sh/uv/) and Node 22+.

```sh
make install     # npm + Python dependencies
make dev         # infra in Docker; gateway, AI service and web app native with hot reload
make ci          # lint, type-check, all tests, generated-contract drift check
make e2e         # Playwright e2e + latency budgets against a running stack
make bench       # criterion benchmarks (incl. 50M-row aggregations) + live gateway latency
make bench-ws    # end-to-end query latency over the data socket (after `make seed-bench`)
make help        # everything else
```

## Layout

```
apps/web/              Next.js 16 app (App Router, Tailwind 4, shadcn/ui)
services/gateway/      Rust: Axum API + WebSocket, auth, RBAC, DuckDB engine, connectors,
                       query planner, result cache, gRPC tool server             (crates/, bin/gateway)
services/ai/           Python: gRPC AI service + probe/metrics HTTP              (uv)
packages/proto/        gRPC contracts (gateway ⇄ AI)
packages/shared-types/ TypeScript generated from the gateway's Rust types (ts-rs)
infra/docker/          Postgres 18 + Apache AGE + pgvector image and init scripts
docs/                  ADRs, phase reports, the frontend design
scripts/               codegen, dev loop, benchmarks
```

## Docs

- [SPEC.md](SPEC.md) — product spec and phase plan
- [ADR 0001](docs/adr/0001-stack.md) stack, service boundaries, performance budgets ·
  [ADR 0002](docs/adr/0002-workspaces-roles-and-logic-scope.md) workspaces, roles, logic scope ·
  [ADR 0003](docs/adr/0003-version-pins-and-deviations.md) version pins and deviations ·
  [ADR 0004](docs/adr/0004-engine-and-connectors.md) query engine, extracts and connectors
- [docs/design](docs/design/) — the UI source of truth
