#!/usr/bin/env bash
# Fast local loop: Postgres, Redis and object storage in Docker; gateway, AI service and
# web app run natively with hot reload. Ctrl+C stops all three.
#
# Needs: Rust (rustup), uv, Node 22+. Rust hot reload uses watchexec or cargo-watch when
# installed (`cargo install watchexec-cli`); otherwise the gateway runs without reload.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# The containerised app services would hold ports 3000/8080; infra stays up.
docker compose stop web ai gateway >/dev/null 2>&1 || true
docker compose up -d --wait postgres redis s3
docker compose up s3-init >/dev/null

export DATABASE_URL="postgres://lattice:lattice@localhost:${POSTGRES_PORT:-5433}/lattice"
export REDIS_URL="redis://localhost:${REDIS_PORT:-6379}"
export S3_ENDPOINT="http://localhost:${S3_PORT:-9000}"
export AI_GRPC_URL="http://localhost:50051"
export TOOLS_GRPC_ADDR="127.0.0.1:50052"
export GATEWAY_TOOLS_URL="localhost:50052"
export NEXT_PUBLIC_GATEWAY_URL="http://localhost:8080"
export SEED_DEMO="${SEED_DEMO:-true}"
export DATA_DIR="${DATA_DIR:-$PWD/.data}"
export S3_ACCESS_KEY="${S3_ACCESS_KEY:-lattice}"
export S3_SECRET_KEY="${S3_SECRET_KEY:-lattice-dev-secret}"
# Local-development key only (same default as docker-compose.yml).
export LATTICE_SECRET_KEY="${LATTICE_SECRET_KEY:-FFcJLROL+t7RwWNo2E3Ks5dSmpC4Detu+wZazB06rNo=}"
export COOKIE_SECURE="${COOKIE_SECURE:-false}"
export RUST_LOG="${RUST_LOG:-info}"

pids=()
cleanup() {
  kill "${pids[@]}" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

(
  cd services/gateway
  if command -v watchexec >/dev/null; then
    exec watchexec --restart --watch crates --watch bin --watch migrations --exts rs,sql,toml -- cargo run --bin gateway
  elif command -v cargo-watch >/dev/null; then
    exec cargo watch --quiet --exec "run --bin gateway"
  else
    echo "dev: install watchexec (cargo install watchexec-cli) for gateway hot reload" >&2
    exec cargo run --bin gateway
  fi
) &
pids+=($!)

(cd services/ai && exec uv run watchfiles --filter python "python -m lattice_ai" src) &
pids+=($!)

(cd apps/web && exec npm run dev) &
pids+=($!)

echo "dev: web http://localhost:3000 · gateway http://localhost:8080 · demo@lattice.dev / lattice-demo"
wait
