# Phase 0 report — Skeleton

- Date: 2026-10-31
- Exit criteria: `docker compose up` works from a clean clone ✅ · CI green — all CI steps pass
  locally (`make ci`, Playwright, benchmarks); the GitHub Actions run is the first push's run.

Measured on an Apple-silicon Mac (10 cores, 16 GB, Docker Desktop with 8 GB), macOS, Docker 29.1.

## What was built

**Infrastructure**
- `docker-compose.yml`: web, gateway, ai, postgres, redis, s3 (RustFS), each with a healthcheck and
  ordered startup (`depends_on: service_healthy`). Host ports are configurable; Postgres is on 5433.
- Postgres 18.6 image with Apache AGE 1.8.0 and pgvector 0.8.7, AGE exposed under
  `$libdir/plugins` so non-superuser roles can `LOAD` it. Init scripts create both extensions and
  the read-only `lattice_ai_ro` role (connect only, no table grants).
- Opt-in `sso` profile with a dev OpenID Connect provider for exercising SSO locally.

**Gateway (Rust)** — crates `telemetry`, `security`, `project`, `aibridge`, `api`, binary `gateway`
(`serve | migrate | seed | healthcheck`).
- Axum API: `/healthz` (no I/O), `/readyz` (Postgres, Redis, AI gRPC health, object storage, probed
  concurrently with 1 s timeouts), auth, projects, WebSocket `/ws` (hello + ping/pong).
- Auth: email/password (Argon2id in `spawn_blocking`; constant-time-ish failure path for unknown
  emails), OIDC for any number of providers (PKCE + nonce + state in Redis + a state cookie binding
  the callback to the browser), sessions as 256-bit opaque tokens stored only as SHA-256,
  `HttpOnly; SameSite=Lax` cookies (Secure configurable), 30-day "keep me signed in" or browser
  sessions (12 h server-side cap), server-side logout revocation, hourly purge.
- Tenancy and RBAC per [ADR 0002](../adr/0002-workspaces-roles-and-logic-scope.md): workspaces,
  project roles Viewer/Editor/Admin with the design's permission matrix, non-members get 404.
- Security middleware: CORS pinned to the web origin, Origin check on state-changing requests and
  on WebSocket upgrades, per-IP rate limit on sign-up/sign-in, request ids, 30 s timeouts,
  compression only above 1 KiB, audit log for sign-ups, sign-ins and project creation.
- Every response carries `Server-Timing` (`auth`, `db`, `rbac`, `total`) plus
  `Timing-Allow-Origin`, so the browser's Resource Timing API can read it.
- sqlx with compile-time-checked queries (offline cache committed), migrations on a dedicated
  connection, pooled connections run `LOAD '$libdir/plugins/age'; SET search_path = ag_catalog, …`.
- gRPC: client for the AI service (lazy connect) and the `GatewayTools` server on :50052 with the
  standard gRPC health service. Code generated at build time with a vendored `protoc`.
- `tracing` with JSON logs in containers and an OTLP/gRPC span exporter when
  `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
- Demo seed: "Acme Retail" workspace, `demo@lattice.dev`, two teammates without passwords, three
  projects from the design.

**AI service (Python)** — async gRPC `AiService.Ask` stub that streams `Progress` immediately,
calls back into `GatewayTools.Ping` (proving the AI → gateway path), and ends with a placeholder
`Answer` or a `Failure` event. FastAPI side-car with `/healthz`, `/readyz` (gateway tool health) and
Prometheus `/metrics`. Settings via pydantic-settings; versioned-prompts directory convention.

**Web (Next.js 16)** — screens 01–03 of the design: sign in, create workspace, all projects (filter
tabs synced to the URL, new-project dialog, cards prefetching the project on hover), project
overview (members live; sheets/sources/digest as honest empty states), a business-logic placeholder
and settings. The prerendered shell paints first; URL-dependent parts stream in behind Suspense.
Perf overlay on **Ctrl+Shift+P** (or `?perf`): TTFB/FCP/LCP/DCL, FPS, long tasks, React commit time
(dev builds), WebSocket RTT, and each gateway request's browser time beside its Server-Timing
phases, all from a non-reactive store.

**Contracts** — `packages/proto` (AiService, GatewayTools) → Rust at build time and committed Python
stubs with typed `.pyi`; 21 Rust API types → TypeScript via ts-rs. `scripts/gen.sh` is
deterministic (verified: identical hashes across runs), and CI fails on drift.

**CI** — `.github/workflows/ci.yml`: gateway (fmt, clippy `-D warnings`, sqlx cache check, nextest
against Postgres, contract drift), AI (ruff, mypy `--strict`, pytest), web (prettier, eslint, tsc,
vitest, production build), benchmarks (PRs fail when a criterion benchmark is >10% slower than the
target branch, comparing the confidence interval's lower bound), and a stack job (clean
`docker compose up --build --wait`, Playwright e2e + latency budgets, gateway latency, timings in
the job summary).

## Numbers

### Clean clone → running stack

The clone contained exactly the files git would ship (206 files, 2.8 MB). `docker compose build
--no-cache` then `docker compose up --wait`, base images already present locally:

| Step | Time |
|---|---|
| Build all images, no cache (Rust release build with thin LTO dominates) | 148 s |
| `up --wait` until all six services report healthy | 17 s |
| **Total** | **165 s** |

On a machine without the base images, add their download (~1.4 GB). Images: gateway 75.6 MB
(distroless), ai 258 MB, web 438 MB, postgres 672 MB.

### Gateway latency

Live stack via Docker Desktop port forwarding, one keep-alive connection, 3,000 requests after warm-up
(`scripts/bench-gateway.py`):

| Endpoint | p50 | p95 | p99 | Inside the gateway (Server-Timing p50) |
|---|---|---|---|---|
| `GET /healthz` | 0.133 ms | 0.157 ms | 0.195 ms | < 0.01 ms |
| `GET /api/projects` (session lookup + list query) | 0.385 ms | 0.508 ms | 0.639 ms | 0.21 ms |
| `GET /readyz` (4 dependency probes) | 1.8–5 ms | | | |

In-process criterion benchmarks (`cargo bench`):

| Benchmark | Time |
|---|---|
| Request through the full middleware stack to `/healthz` | 5.19 µs |
| Unauthenticated request rejected (cookie check + JSON error) | 5.97 µs |
| Server-Timing: record 3 phases + render header | 158 ns |
| Session token: generate / hash for lookup | 732 ns / 112 ns |
| Argon2id verify (deliberately slow; bounds sign-in latency) | 10.9 ms |

Against the "< 5 ms gateway overhead on a cached query" budget, the fixed per-request cost is
about 0.1% of it.

### Web first load (production build, Playwright/Chromium, warm HTTP cache)

| Page | TTFB | FCP | DCL | Data visible |
|---|---|---|---|---|
| `/login` | 2 ms | 28 ms | 8.4 ms | — (static) |
| `/projects` (signed in) | 8.2 ms | 48 ms | 31 ms | 99.5 ms after navigation start |
| Project switch, prefetched on hover (client navigation → members rendered) | | | | 65 ms |

No main-thread task over 50 ms on either page (asserted). First-load JavaScript: `/login` 227 KB
gzip (12 scripts, ~170 KB of it React 19 + the Next.js runtime), `/projects` 251 KB gzip.

### Tests

| Suite | Count | Result |
|---|---|---|
| Rust (unit + gRPC round trip + 10 HTTP integration tests on real Postgres) | 55 | pass |
| Python (gRPC streaming, failure paths, probes, settings) | 10 | pass |
| Web unit (Vitest + Testing Library) | 21 | pass |
| Playwright e2e + latency budgets | 6 | pass |
| SSO end to end (browser → dev IdP → callback → new user + workspace + audit entry) | manual script | pass |

## Decisions made during the phase

Asked (spec vs design contradictions), answered by the product owner — see ADR 0002:
1. Business logic is **workspace-wide plus project-local**.
2. Roles are the design's **Viewer / Editor / Admin** (+ workspace Admin/Member).

Made without asking (ADR 0003 has the details): Lattice naming; RustFS instead of MinIO (MinIO no
longer publishes images); Postgres on host port 5433; sqlx 0.8; tower-http 0.6 (dedupe);
npm workspaces; clsx + tailwind-merge instead of the day-old `cn` package; generated code
committed; "SSO" not "SAML"; the dev IdP profile; truthful sign-in footer copy; gateway crates
created when their phase needs them.

## Open issues and follow-ups

1. **WebSocket sessions aren't re-validated** after the upgrade; a revoked or expired session keeps
   an open socket. Phase 1 (when the socket carries data): periodic re-check or revocation via Redis.
2. **Rate limiting keys on the peer IP.** Behind a load balancer every client shares one IP; add
   trusted-proxy `X-Forwarded-For` handling before production (phase 6).
3. **OIDC links accounts by verified email** for every provider. Restrict linking to providers
   marked trusted before production; add workspace invitations / domain join (SSO users currently
   get a new workspace).
4. **No password reset** (needs email delivery); the "Forgot password?" link explains this.
5. **OTLP export is wired but untested against a live collector** (none in Compose). Add an
   optional Jaeger/OTel-collector profile and propagate trace context gateway → AI over gRPC.
6. `NEXT_PUBLIC_GATEWAY_URL` is fixed at build time; a runtime config endpoint is needed before one
   image serves several environments.
7. CI builds Docker images without a layer cache; add buildx `cache-from: gha` if the stack job is slow.
8. The 10% benchmark gate runs on shared GitHub runners; if it proves noisy, move it to a dedicated
   runner rather than loosen the threshold.
9. Ctrl+Shift+P opens a private window in Firefox on Windows/Linux; `?perf` works everywhere. React
   commit times appear only in development builds.
10. Web image is 438 MB (Node slim + standalone output); a distroless Node base would roughly halve it.
11. `make seed` creates users, the workspace and projects; the demo dataset (phase 1) and
    pre-approved demo business logic (phase 3) join it in their phases.
12. Project switching is 65 ms with prefetch, mostly Next.js's client navigation to a dynamic
    route. Sheet switching (phase 2, budget < 100 ms) should keep sheet state client-side rather than
    navigate between server routes.

## Ready for phase 1

Engine and connectors: DuckDB behind an `Engine` trait, Arrow IPC frames on the existing `/ws`,
file upload to object storage, Postgres/MySQL/SQLite connectors, introspection and profiling, the
Redis result cache and end-to-end cancellation. Exit criterion: 50M-row Parquet aggregations
< 100 ms warm in the gateway.
