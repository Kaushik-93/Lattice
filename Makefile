# Lattice developer commands. `make help` lists them.
SHELL := /bin/bash
export PATH := $(HOME)/.cargo/bin:$(PATH)

POSTGRES_PORT ?= 5433
TEST_DATABASE_URL ?= postgres://lattice:lattice@localhost:$(POSTGRES_PORT)/lattice
GATEWAY := services/gateway
AI := services/ai
WEB := apps/web

.DEFAULT_GOAL := help
.PHONY: help up down logs dev seed gen sqlx-prepare install lint test test-gateway test-ai test-web e2e bench ci clean

help: ## List commands
	@grep -E '^[a-z0-9-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2}'

up: ## Build and start the whole stack in Docker (http://localhost:3000)
	docker compose up -d --build --wait

down: ## Stop the stack (data volumes are kept)
	docker compose down

logs: ## Follow logs of all services
	docker compose logs -f --tail=100

dev: ## Infra in Docker, gateway/AI/web native with hot reload
	./scripts/dev.sh

seed: ## Create the demo workspace (demo@lattice.dev / lattice-demo) if missing
	docker compose exec gateway /usr/local/bin/gateway seed

gen: ## Regenerate cross-language contracts (proto → Python, Rust → TypeScript)
	./scripts/gen.sh

sqlx-prepare: ## Refresh the offline sqlx query cache (needs Postgres from `make up` or `make dev`)
	cd $(GATEWAY) && DATABASE_URL=$(TEST_DATABASE_URL) sqlx migrate run && \
		SQLX_OFFLINE=false DATABASE_URL=$(TEST_DATABASE_URL) cargo sqlx prepare --workspace -- --all-targets

install: ## Install JS and Python dependencies
	npm ci
	cd $(AI) && uv sync --locked

lint: ## Format checks, clippy, ruff, mypy, eslint, tsc
	cd $(GATEWAY) && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
	cd $(AI) && uv run --locked ruff format --check . && uv run --locked ruff check . && uv run --locked mypy
	cd $(WEB) && npm run format:check && npm run lint && npm run typecheck

test: test-gateway test-ai test-web ## All unit and integration tests

test-gateway: ## Rust tests (integration tests need Postgres: `make up` or `make dev`)
	cd $(GATEWAY) && DATABASE_URL=$(TEST_DATABASE_URL) cargo nextest run --workspace

test-ai:
	cd $(AI) && uv run --locked pytest -q

test-web:
	cd $(WEB) && npm test

e2e: ## Playwright e2e + latency-budget tests against the running stack
	cd $(WEB) && npx playwright test

bench: ## Criterion micro-benchmarks + live gateway latency
	cd $(GATEWAY) && cargo bench --workspace --bench '*'
	python3 scripts/bench-gateway.py

ci: lint test ## What CI runs before the Docker/e2e job, plus a contract drift check
	./scripts/gen.sh && git diff --exit-code -- packages/shared-types $(AI)/src/lattice

clean: ## Remove build artefacts (keeps Docker volumes)
	rm -rf $(GATEWAY)/target $(WEB)/.next $(WEB)/test-results $(WEB)/playwright-report perf-results
