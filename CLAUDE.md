# Lattice — notes for coding agents

GenAI-native BI platform. Read [SPEC.md](SPEC.md) (the product spec and phase plan), the ADRs in
[docs/adr](docs/adr/) (decisions that amend the spec), and the latest report in
[docs/phases](docs/phases/). The UI follows [docs/design](docs/design/) — match it.

## Map

- `apps/web` — Next.js 16 app (client components for anything interactive). Read
  `node_modules/next/dist/docs/` before using Next APIs: v16 differs from older versions
  (cacheComponents is on; URL-reading client components need Suspense).
- `services/gateway` — Rust workspace: `crates/{api,project,security,aibridge,telemetry}`, `bin/gateway`.
- `services/ai` — Python gRPC service (uv). Generated stubs in `src/lattice/` — don't edit.
- `packages/proto` — gRPC contracts. `packages/shared-types` — TS generated from Rust — don't edit.

## Commands

`make help` lists everything. Common: `make up` (whole stack in Docker), `make dev` (hot reload),
`make ci` (lint + tests + contract drift), `make e2e`, `make bench`, `make gen` after changing a
`.proto` or a `#[derive(TS)]` type, `make sqlx-prepare` after changing a `sqlx::query!`.

## Rules that are easy to break

- The browser talks to the gateway directly; never proxy data through Next.js.
- The AI service never touches a database or data source; it calls gateway tools over gRPC.
- Rust: no `unsafe`, no `unwrap()` outside tests, `thiserror` in crates / `anyhow` in the binary,
  `clippy -D warnings`. Queries use `sqlx::query!` macros (offline cache in `.sqlx/`, committed).
- Pooled Postgres connections put `ag_catalog` first on the search_path; migrations run on a
  separate connection so tables land in `public`.
- TypeScript strict with no `any`; Python `mypy --strict`. Prompts live in versioned files.
- Every significant decision or deviation gets an ADR. Performance budgets (spec §3) are
  enforced by criterion benches and Playwright tests; keep them green.
