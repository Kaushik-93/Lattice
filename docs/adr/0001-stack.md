# ADR 0001 — Stack, service boundaries and performance budgets

- Status: accepted
- Date: 2026-10-31
- Context: Phase 0 of [SPEC.md](../../SPEC.md). The product is named **Lattice** (the spec's
  working name was "Lumen"); identifiers use `lattice` (`lattice_ai`, `.lattice` archives, the
  `lattice` database).

## Decision

Lattice is three services plus managed infrastructure. Each language does the job it is best at,
and every boundary is a generated contract.

| Service | Language | Owns |
|---|---|---|
| `apps/web` | TypeScript, Next.js 16 (App Router), React 19, Tailwind 4, shadcn/ui (Radix), Zustand, TanStack Query | UI shell, routing, the sheet canvas (client components). **No data passes through Next.js.** |
| `services/gateway` | Rust 2024, Axum 0.8, tokio, sqlx 0.8, tonic 0.14 | Auth and sessions, RBAC on every call, the API (REST + WebSocket), query planning and execution (phase 1+), the business-logic layer (phase 3+), audit log, the gRPC tool server the AI calls |
| `services/ai` | Python 3.13, uv, grpcio, FastAPI (probes only), Pydantic 2 | The LLM tool loop (phase 4), proposals, embeddings, later forecasting |
| Postgres 18 + Apache AGE + pgvector | — | Metadata, business logic, audit (relational source of truth); graph projection; embeddings |
| Redis 7.4 | — | Hot result cache (phase 1), pub/sub, short-lived auth state (OIDC) |
| S3-compatible object storage | — | Uploaded files, Parquet extracts. Local dev: RustFS (see ADR 0003) |

Exact versions and the reasons for each pin are in [ADR 0003](0003-version-pins-and-deviations.md).

### Service boundaries

```
Browser ──REST (JSON) + WebSocket (JSON control, Arrow IPC from phase 1)──▶ Gateway
Gateway ──gRPC AiService.Ask (server-streaming)──▶ AI service
AI service ──gRPC GatewayTools.*──▶ Gateway          (the ONLY way the AI reaches data or logic)
Gateway ──sqlx──▶ Postgres (+AGE, +pgvector)   ──redis──▶ Redis   ──S3──▶ object storage
```

1. **The browser talks to the gateway directly.** Next.js serves the app shell and static assets;
   it never proxies data or API calls (the only route handler is `/api/health`). This removes a hop
   from every interaction and keeps one place for auth.
2. **The AI service never touches a data source or database directly.** It calls the gateway's
   `GatewayTools` gRPC API for everything: semantic search, graph context, previews, proposals.
   RBAC, row-level security, approval status and auditing are therefore enforced in exactly one
   place, and the AI cannot bypass them by construction. The only exception the spec allows is a
   read-only Postgres role (`lattice_ai_ro`) for embedding search; in phase 0 it can connect but has
   no table grants. If granting it the embeddings table turns out to be awkward in phase 4, that
   search moves behind a gateway tool and the role is dropped.
3. **The gateway owns authentication.** OIDC (any compliant provider) and email/password with
   Argon2id; an opaque 256-bit session token in an `HttpOnly; SameSite=Lax` cookie (`Secure` outside
   local http); only the token's SHA-256 is stored. Every handler resolves the caller through one
   indexed query and checks project roles before touching project data.
4. **Contracts are generated, never hand-copied.** `packages/proto` → Rust (tonic, at build time) and
   Python (committed stubs with typed `.pyi`); Rust API types → TypeScript (ts-rs, committed).
   `scripts/gen.sh` regenerates everything and CI fails on drift.

### Performance budgets (spec §3, non-negotiable)

| Interaction | Budget (p95) | Phase 0 status |
|---|---|---|
| First load of a sheet, warm cache | < 1.2 s to interactive; skeletons within 100 ms | App-shell pages measured (see phase report); sheets arrive in phase 2 |
| Switching sheets | < 100 ms (prefetched) | Project switch measured (prefetch on hover) |
| Cross-filter → all visuals updated (≤ 50M rows) | < 150 ms incl. network | Phase 2 |
| Pan / zoom / hover | 60 fps; no main-thread task > 50 ms | Long tasks asserted absent in Playwright |
| Gateway overhead on a cached query | < 5 ms | In-process middleware cost benchmarked; cache in phase 1 |
| NL question → first chart, our overhead | < 200 ms | Phase 4 |
| NL question → first chart end to end | < 3 s, streamed progress from 0 ms | Streaming contract in place (`AskEvent` stream) |
| Schema introspection, 500 tables | < 5 s, then cached | Phase 1 |

Rules that follow from the budgets and are already enforced in phase 0:

- Every gateway response carries a `Server-Timing` header (phases such as `auth`, `db`, `rbac`,
  plus `total`), readable cross-origin via `Timing-Allow-Origin`; the perf overlay (Ctrl+Shift+P)
  shows it next to the browser-observed time, WebSocket RTT, page vitals, long tasks and FPS.
- Performance samples live in a non-reactive store; recording never re-renders the app.
- URL-dependent UI sits behind Suspense so the static shell and skeletons prerender and paint first.
- Large data never enters React state (phase 1 keeps Arrow tables in refs / a non-reactive store).
- Criterion benchmarks cover gateway hot paths; CI fails a PR when a benchmark regresses by more
  than 10% (whole confidence interval past the threshold). Playwright tests assert the budgets.

## Consequences

- Three toolchains (Rust, Python/uv, Node) are needed to develop locally; `docker compose up` needs
  only Docker.
- The gateway is the single choke point for security and performance. It is also the part that
  needs the most care (benchmarks, `forbid(unsafe_code)`, `clippy -D warnings`, no `unwrap()`).
- Gateway crates are added as their phase starts (`engine`, `connectors`, `catalog`, `cache` in
  phase 1; `planner` in 2; `semantic`, `graph` in 3) instead of as empty placeholders now.
