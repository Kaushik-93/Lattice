# syntax=docker/dockerfile:1.7
# Build context: repository root (the build script compiles ../../packages/proto).

FROM rust:1.99.1-slim-trixie AS chef
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /src/services/gateway

FROM chef AS planner
COPY services/gateway/ ./
RUN cargo chef prepare --recipe-path recipe.json

# Dependencies compile in their own layer, keyed only on Cargo.toml/Cargo.lock, so editing
# gateway code doesn't rebuild tonic/sqlx/axum.
FROM chef AS builder
COPY packages/proto /src/packages/proto
COPY --from=planner /src/services/gateway/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY services/gateway/ ./
# sqlx macros check queries against the committed .sqlx/ cache; no database at build time.
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked --bin gateway \
 && mkdir -p /out/data \
 && cp target/release/gateway /out/gateway

# Distroless: glibc + libgcc only, no shell, no package manager, runs as uid 65532.
FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=builder /out/gateway /usr/local/bin/gateway
# Pre-create the data dir owned by nonroot; a fresh named volume inherits this ownership.
COPY --from=builder --chown=65532:65532 /out/data /data
ENV BIND_ADDR=0.0.0.0:8080 \
    TOOLS_GRPC_ADDR=0.0.0.0:50052 \
    DATA_DIR=/data
EXPOSE 8080 50052
ENTRYPOINT ["/usr/local/bin/gateway"]
CMD ["serve"]
