//! Gateway overhead with no I/O: a request through the full middleware stack (request id,
//! tracing, timeout, CORS, compression check, Server-Timing, origin guard) to a handler.
//! The §3 budget for a cached query is < 5 ms; this is the floor everything builds on.

#![allow(clippy::expect_used)]

use std::hint::black_box;
use std::net::SocketAddr;

use axum::Extension;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use criterion::{Criterion, criterion_group, criterion_main};
use lattice_api::{AppState, Config};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt as _;

fn bench(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let app = runtime.block_on(async {
        let config = Config::from_lookup(|key| match key {
            "DATABASE_URL" => Some("postgres://bench@127.0.0.1:1/none".into()),
            "REDIS_URL" => Some("redis://127.0.0.1:1".into()),
            "AI_GRPC_URL" => Some("http://127.0.0.1:1".into()),
            _ => None,
        })
        .expect("config");
        // Lazy pool: never connects, which is fine for routes that don't touch the database.
        let pool = PgPoolOptions::new()
            .connect_lazy(&config.database_url)
            .expect("lazy pool");
        lattice_api::router(AppState::new(config, pool).expect("state"))
            // A real ConnectInfo, not MockConnectInfo: the rate limiter reads the raw extension.
            .layer(Extension(ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                40000,
            )))))
    });

    c.bench_function("router/healthz_full_stack", |b| {
        b.to_async(&runtime).iter(|| async {
            let req = Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .expect("request");
            black_box(app.clone().oneshot(req).await.expect("response"))
        });
    });

    c.bench_function("router/unauthenticated_rejection", |b| {
        b.to_async(&runtime).iter(|| async {
            let req = Request::builder()
                .uri("/api/projects")
                .header(header::ORIGIN, "http://localhost:3000")
                .body(Body::empty())
                .expect("request");
            black_box(app.clone().oneshot(req).await.expect("response"))
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
