//! Phase 1 exit criterion: 50M-row aggregations < 100 ms warm in the gateway.
//!
//! Measured through the engine API (semaphore, pooled connection, blocking thread, DuckDB,
//! Arrow materialisation) with planner-compiled SQL, on the generated retail dataset. The
//! database is built once under Cargo's target directory and reused. Override the size with
//! `LATTICE_BENCH_ROWS`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use lattice_engine::{Engine, EngineConfig, ProjectDb, demo, ipc};
use lattice_planner::{ColumnInfo, CompiledQuery, TableQuery, TableSchema, compile};
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SCHEMA: &str = "bench";

fn rows() -> u64 {
    std::env::var("LATTICE_BENCH_ROWS")
        .ok()
        .and_then(|v| v.replace('_', "").parse().ok())
        .unwrap_or(50_000_000)
}

fn setup(rt: &Runtime) -> (Arc<ProjectDb>, TableSchema) {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let rows = rows();
    let engine = Engine::new(EngineConfig {
        data_dir: root.join("bench-data"),
        extension_dir: root.join("duckdb-extensions"),
        memory_limit: "6GB".into(),
        threads: None,
        pool_size: 4,
        max_open_projects: 1,
        query_timeout: Duration::from_secs(120),
    });
    // A fixed project id per size, so the generated database survives between runs.
    let project = Uuid::from_u128(u128::from(rows));
    rt.block_on(async {
        engine.install_extensions().await.expect("extensions");
        let db = engine.open(project, vec![]).await.expect("open");
        let existing = db
            .query(
                format!("SELECT count(*) FROM \"{SCHEMA}\".\"orders\""),
                vec![],
                &CancellationToken::new(),
            )
            .await;
        if existing.map(|o| o.rows).unwrap_or(0) == 0 {
            let started = Instant::now();
            let customers = (rows / 20).max(1000);
            db.create_table_as(
                SCHEMA.into(),
                "orders".into(),
                demo::orders_sql(rows, customers),
            )
            .await
            .expect("generate");
            eprintln!("generated {rows} orders in {:?}", started.elapsed());
        }
        let tables = db
            .introspect("project".into(), Some(SCHEMA.into()))
            .await
            .expect("introspect");
        let orders = tables
            .into_iter()
            .find(|t| t.name == "orders")
            .expect("orders table");
        let schema = TableSchema {
            reference: format!("\"project\".\"{SCHEMA}\".\"orders\""),
            columns: orders
                .columns
                .iter()
                .map(|c| ColumnInfo {
                    name: c.name.clone(),
                    data_type: c.data_type.clone(),
                })
                .collect(),
        };
        // Leak the engine: the database must outlive the runtime's benchmark loop.
        let engine = Box::leak(Box::new(engine));
        (engine.get(project).await.expect("open project"), schema)
    })
}

fn query(schema: &TableSchema, json: &str) -> CompiledQuery {
    let q: TableQuery = serde_json::from_str(&format!(
        r#"{{"table_id":"00000000-0000-0000-0000-000000000000",{json}}}"#
    ))
    .expect("query json");
    compile(&q, schema).expect("compiles")
}

fn bench(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let (db, schema) = setup(&rt);
    let cases = [
        (
            "sum_amount_by_region",
            r#""dimensions":[{"column":"region"}],"measures":[{"column":"amount","agg":"sum"}]"#,
        ),
        (
            "sum_amount_by_month",
            r#""dimensions":[{"column":"order_date","bucket":"month"}],"measures":[{"column":"amount","agg":"sum"},{"agg":"count"}]"#,
        ),
        (
            "top10_categories_filtered",
            r#""dimensions":[{"column":"category"}],"measures":[{"column":"amount","agg":"sum"}],"filters":[{"column":"order_date","op":"gte","value":"2025-01-01"}],"sort":[{"by":"sum_amount","dir":"desc"}],"limit":10"#,
        ),
        (
            "sum_avg_by_store_and_region",
            r#""dimensions":[{"column":"store_id"},{"column":"region"}],"measures":[{"column":"amount","agg":"sum"},{"column":"amount","agg":"avg"}]"#,
        ),
        (
            "filtered_count_one_region",
            r#""measures":[{"agg":"count"},{"column":"amount","agg":"sum"}],"filters":[{"column":"region","op":"eq","value":"EMEA"},{"column":"category","op":"eq","value":"Outdoor"}]"#,
        ),
        (
            "approx_distinct_customers_by_region",
            r#""dimensions":[{"column":"region"}],"measures":[{"column":"customer_id","agg":"approx_count_distinct"}]"#,
        ),
        (
            "exact_distinct_customers_by_region",
            r#""dimensions":[{"column":"region"}],"measures":[{"column":"customer_id","agg":"count_distinct"}]"#,
        ),
    ];
    let label = format!("{}M", rows() / 1_000_000);
    let mut group = c.benchmark_group(format!("engine_{label}_rows"));
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));
    for (name, json) in cases {
        let compiled = query(&schema, json);
        group.bench_function(name, |b| {
            b.to_async(&rt).iter(|| {
                let (db, sql, params) = (db.clone(), compiled.sql.clone(), compiled.params.clone());
                async move {
                    db.query(sql, params, &CancellationToken::new())
                        .await
                        .expect("query")
                }
            });
        });
    }
    group.finish();

    // Arrow IPC encoding of a typical chart result (33 monthly rows), incl. decimal → float.
    let month = query(&schema, cases[1].1);
    let out = rt
        .block_on(db.query(month.sql, month.params, &CancellationToken::new()))
        .expect("month query");
    c.bench_function("engine/ipc_encode_monthly_result", |b| {
        b.iter(|| {
            let (schema, batches) =
                ipc::for_browser(&out.schema, out.batches.clone()).expect("normalise");
            ipc::encode(&schema, &batches).expect("encode")
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
