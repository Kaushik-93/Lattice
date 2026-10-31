# Lattice

**Ask your data. Keep the logic.** A GenAI-native business-intelligence platform: connect data
sources, ask questions in plain language, get interactive charts — built on a governed,
human-approved business-logic layer, so every number is traceable to a definition someone signed
off.

Status: **phase 0 (skeleton)** — auth, workspaces, projects, the app shell from the design, the
Rust gateway, the Python AI service stub, generated contracts, perf instrumentation and CI.
See [docs/phases/PHASE_0_REPORT.md](docs/phases/PHASE_0_REPORT.md).

## Run it

Requires Docker.

```sh
docker compose up --build        # or: make up
```

Open http://localhost:3000 and sign in as **demo@lattice.dev / lattice-demo** (seeded "Acme Retail"
workspace), or create your own workspace. Press **Ctrl+Shift+P** for the performance overlay.

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
make bench       # criterion benchmarks + live gateway latency
make help        # everything else
```

## Layout

```
apps/web/              Next.js 16 app (App Router, Tailwind 4, shadcn/ui)
services/gateway/      Rust: Axum API + WebSocket, auth, RBAC, gRPC tool server   (crates/, bin/gateway)
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
  [ADR 0003](docs/adr/0003-version-pins-and-deviations.md) version pins and deviations
- [docs/design](docs/design/) — the UI source of truth
