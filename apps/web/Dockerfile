# syntax=docker/dockerfile:1.7
# Build context: repository root (npm workspaces: apps/web, packages/shared-types, packages/wasm).

FROM node:24.22.2-trixie-slim AS deps
WORKDIR /repo
COPY package.json package-lock.json ./
COPY apps/web/package.json apps/web/
COPY packages/shared-types/package.json packages/shared-types/
COPY packages/wasm/package.json packages/wasm/
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund

# Hot-reload target used by `make dev-docker` (source is bind-mounted over /repo/apps/web).
FROM deps AS dev
COPY packages/shared-types packages/shared-types
COPY packages/wasm/pkg packages/wasm/pkg
ENV NEXT_TELEMETRY_DISABLED=1
WORKDIR /repo/apps/web
CMD ["npx", "next", "dev", "--hostname", "0.0.0.0"]

FROM deps AS build
COPY packages/shared-types packages/shared-types
# The compute worker's WebAssembly, prebuilt and committed (scripts/build-wasm.sh).
COPY packages/wasm/pkg packages/wasm/pkg
COPY apps/web apps/web
# NEXT_PUBLIC_* values are inlined into the client bundle at build time.
ARG NEXT_PUBLIC_GATEWAY_URL=http://localhost:8080
ENV NEXT_PUBLIC_GATEWAY_URL=${NEXT_PUBLIC_GATEWAY_URL} \
    NEXT_TELEMETRY_DISABLED=1
RUN npm run build --workspace @lattice/web

FROM node:24.22.2-trixie-slim AS runtime
WORKDIR /app
ENV NODE_ENV=production \
    NEXT_TELEMETRY_DISABLED=1 \
    PORT=3000 \
    HOSTNAME=0.0.0.0
# Standalone output mirrors the monorepo layout; static assets are copied beside it.
COPY --from=build --chown=node:node /repo/apps/web/.next/standalone ./
COPY --from=build --chown=node:node /repo/apps/web/.next/static ./apps/web/.next/static
COPY --from=build --chown=node:node /repo/apps/web/public ./apps/web/public
USER node
EXPOSE 3000
CMD ["node", "apps/web/server.js"]
