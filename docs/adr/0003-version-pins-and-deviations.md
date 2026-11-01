# ADR 0003 — Version pins and deviations from the spec

- Status: accepted
- Date: 2026-10-31 (every version below was checked against its registry on this date)

## Pins

### Container images

| Image | Pin | Notes |
|---|---|---|
| Postgres | `postgres:18.6-trixie` | Debian 13 base, PGDG repo |
| Apache AGE | `postgresql-18-age=1.8.0~rc0-2.pgdg13+1` | AGE tags **every** release `-rc0`; `PG18/v1.8.0-rc0` is the GA release (2026-07-09, not a pre-release on GitHub) |
| pgvector | `postgresql-18-pgvector=0.8.7-1.pgdg13+1` | |
| Redis | `redis:7.4.12-alpine` | 7.x as specified (8.x exists) |
| Object storage | `rustfs/rustfs:1.0.3` | replaces MinIO, see below |
| Rust build | `rust:1.99.1-slim-trixie` + `cargo-chef 0.1.78` | |
| Gateway runtime | `gcr.io/distroless/cc-debian13:nonroot` | no shell, uid 65532; 309 MB with DuckDB and its extensions (75.6 MB in phase 0, see ADR 0004) |
| Python | `python:3.13.18-slim-trixie` + `ghcr.io/astral-sh/uv:0.8.19` | uv matches the version that wrote `uv.lock` |
| Node | `node:24.22.2-trixie-slim` | Node 24 is the active LTS |
| Dev IdP (profile `sso`) | `ghcr.io/navikt/mock-oauth2-server:6.0.0` | local SSO testing only |

PGDG removes superseded package versions from its main repo. When a Postgres image build fails on
`apt-get install`, bump `AGE_VERSION`/`PGVECTOR_VERSION` in `infra/docker/postgres/Dockerfile`
(and the image tag) together, then rerun the AGE smoke test from the phase report.

### Toolchains and libraries

- Rust 1.99.1 (`rust-toolchain.toml`), edition 2024. Crate versions are locked in `Cargo.lock`;
  majors are pinned in `Cargo.toml` (axum 0.8, tonic/prost 0.14, sqlx 0.8, tower-http 0.6,
  opentelemetry 0.33, redis 1.x, ts-rs 12, openidconnect 4).
- Python ≥ 3.12 (3.13 in containers); grpcio/grpcio-tools 1.85, protobuf 7.36, FastAPI 0.162,
  Pydantic 2.14, mypy 2.4 (`--strict`), ruff 0.17. Locked in `services/ai/uv.lock`.
- Next.js 16.4.3, React 19.3, Tailwind 4, TypeScript 5.9, TanStack Query 5, Zustand 5, Vitest 5,
  Playwright 1.64. Locked in `package-lock.json`.

Added in phase 1 (checked 2026-11-01):

- Gateway: `duckdb ~1.10506.1` (DuckDB 1.5.6, tilde-pinned because the crate encodes DuckDB's
  version in its minor), arrow 58 (DuckDB's), object_store 0.12 (S3 only), calamine 0.37 (Excel
  sheet listing), aes-gcm 0.10, insta 1 (planner SQL snapshots), tokio-tungstenite 0.29 (tests).
- Web: apache-arrow 21.2, @tanstack/react-table 8.21, @tanstack/react-virtual 3.14.

## Deviations from SPEC.md

1. **MinIO → RustFS.** MinIO stopped publishing container images (`minio/minio` no longer exists
   on Docker Hub; quay.io requires auth). RustFS 1.0 is Apache-2.0, S3-compatible and a drop-in on
   the same ports (9000 API, 9001 console). The Compose service is called `s3`, and the gateway sees
   only `S3_ENDPOINT`, so swapping in SeaweedFS, Garage or real S3 is configuration.
2. **Postgres host port 5433** (container port stays 5432). A locally installed Postgres commonly
   holds 5432, as it does on the machine this was built on; every host port is overridable in `.env`.
3. **sqlx 0.8, not 0.9.** 0.9 has been out five months; 0.8.6 is mature and its offline-cache
   workflow is proven. Upgrade in a dedicated change.
4. **tower-http 0.6**, the version `reqwest` (via openidconnect) already pulls in, so the gateway
   compiles one tower-http instead of two. **opentelemetry-otlp** is built with only the gRPC
   exporter for the same reason (its defaults pull a second reqwest).
5. **npm workspaces** instead of pnpm (pnpm is not installed on the reference machine; npm
   workspaces cover a two-package monorepo).
6. **`clsx` + `tailwind-merge` instead of the `cn` package** that shadcn 4.23's CLI now installs.
   `cn@0.5.1` is genuine (shadcn's own repo) but was one day old and pre-1.0, and it would ship in
   every client bundle; the established pair does the same job.
7. **Generated code is committed** (Python stubs, TypeScript types) with a CI drift check, so a
   fresh clone builds and type-checks without running every generator. Rust gRPC code is generated
   at build time with a vendored `protoc`, so no system protoc is needed anywhere.
8. **"Single sign-on (SAML)" in the design is "Single sign-on (SSO)"**: generic OIDC. SAML is not
   in the spec; enterprise IdPs (Okta, Entra ID, Google Workspace) all speak OIDC.
9. **Next.js 16 conventions**: `cacheComponents` and `partialPrefetching` are on (they become the
   default in the next major). URL-dependent client components sit behind Suspense, which is also
   what makes skeletons paint from prerendered HTML.
10. **Dev IdP in Compose** under the opt-in `sso` profile. It runs in the gateway's network
    namespace so `http://localhost:8090` is one issuer URL for browser and gateway alike.
11. **Sign-in footer copy**: the design's "SOC 2 controls · SSO & SCIM" claims are not true of the
    product yet, so the page states what is ("Governed business logic · Human-approved definitions
    · Every answer traceable"). Restore the original copy once those controls exist.
