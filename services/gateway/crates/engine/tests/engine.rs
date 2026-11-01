//! Engine integration tests on real DuckDB. Extensions are downloaded once into Cargo's
//! test temp directory (network needed on the first run only).

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use duckdb::Connection;
use lattice_connectors::{FileFormat, attach_sqlite};
use lattice_engine::{Engine, EngineConfig, EngineError, Registration};
use lattice_planner::{ColumnInfo, ParamValue, TableQuery, TableSchema, compile};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn engine(timeout: Duration) -> Engine {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    Engine::new(EngineConfig {
        data_dir: root.join(format!("engine-test-{}", Uuid::new_v4())),
        extension_dir: root.join("duckdb-extensions"),
        memory_limit: "1GB".into(),
        threads: Some(4),
        pool_size: 4,
        max_open_projects: 4,
        query_timeout: timeout,
    })
}

async fn ready_engine(timeout: Duration) -> Engine {
    let engine = engine(timeout);
    engine.install_extensions().await.unwrap();
    engine
}

const CSV: &str = "order_date,region,amount\n2026-01-05,EMEA,10.50\n2026-01-20,EMEA,4.50\n2026-02-03,AMER,7.00\n2026-02-10,APAC,3.25\n";

#[tokio::test]
async fn loads_a_csv_extract_and_answers_compiled_queries() {
    let engine = ready_engine(Duration::from_secs(10)).await;
    let db = engine.open(Uuid::new_v4(), vec![]).await.unwrap();
    let path = db.uploads_dir().join("Orders Q1.csv");
    std::fs::write(&path, CSV).unwrap();

    let loaded = db
        .load_file(path, FileFormat::Csv, "s_test".into())
        .await
        .unwrap();
    assert_eq!(loaded, vec![("orders_q1".to_owned(), 4)]);

    let tables = db
        .introspect("project".into(), Some("s_test".into()))
        .await
        .unwrap();
    assert_eq!(tables.len(), 1);
    let columns: Vec<(String, String)> = tables[0]
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();
    assert_eq!(columns[0], ("order_date".into(), "DATE".into()));
    assert_eq!(columns[2].0, "amount");

    let schema = TableSchema {
        reference: "\"project\".\"s_test\".\"orders_q1\"".into(),
        columns: tables[0]
            .columns
            .iter()
            .map(|c| ColumnInfo {
                name: c.name.clone(),
                data_type: c.data_type.clone(),
            })
            .collect(),
    };
    let query: TableQuery = serde_json::from_str(&format!(
        r#"{{"table_id":"{}","dimensions":[{{"column":"order_date","bucket":"month"}}],
            "measures":[{{"column":"amount","agg":"sum"}},{{"agg":"count"}}],
            "filters":[{{"column":"region","op":"neq","value":"APAC"}}]}}"#,
        Uuid::new_v4()
    ))
    .unwrap();
    let compiled = compile(&query, &schema).unwrap();
    let out = db
        .query(compiled.sql, compiled.params, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(out.rows, 2, "January and February");
    let ipc = lattice_engine::ipc::encode(&out.schema, &out.batches).unwrap();
    assert!(ipc.len() > 100);
}

#[tokio::test]
async fn sql_cannot_read_outside_the_project_directory() {
    let engine = ready_engine(Duration::from_secs(10)).await;
    let db = engine.open(Uuid::new_v4(), vec![]).await.unwrap();
    for sql in [
        "SELECT * FROM read_csv('/etc/passwd')",
        "SET enable_external_access = true",
        "INSTALL httpfs",
    ] {
        let err = db
            .query(sql.into(), vec![], &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            matches!(err, EngineError::Query(_)),
            "{sql} must be refused, got {err:?}"
        );
    }
}

const HEAVY: &str =
    "SELECT count(*) FROM range(200000000) a, range(1000) b WHERE a.range % 7 = b.range";

#[tokio::test]
async fn cancellation_interrupts_a_running_query() {
    let engine = ready_engine(Duration::from_secs(30)).await;
    let db = engine.open(Uuid::new_v4(), vec![]).await.unwrap();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        trigger.cancel();
    });
    let started = Instant::now();
    let err = db.query(HEAVY.into(), vec![], &cancel).await.unwrap_err();
    assert!(matches!(err, EngineError::Cancelled), "{err:?}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "stopped promptly: {:?}",
        started.elapsed()
    );

    // The connection went back to the pool and still works.
    let out = db
        .query("SELECT 42".into(), vec![], &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(out.rows, 1);
}

#[tokio::test]
async fn timeouts_interrupt_long_queries() {
    let engine = ready_engine(Duration::from_millis(200)).await;
    let db = engine.open(Uuid::new_v4(), vec![]).await.unwrap();
    let started = Instant::now();
    let err = db
        .query(HEAVY.into(), vec![], &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::Timeout(_)), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn parameters_are_bound_not_interpolated() {
    let engine = ready_engine(Duration::from_secs(10)).await;
    let db = engine.open(Uuid::new_v4(), vec![]).await.unwrap();
    let out = db
        .query(
            "SELECT ?::VARCHAR AS s, ?::BIGINT AS i".into(),
            vec![
                ParamValue::Text("x'); DROP TABLE t; --".into()),
                ParamValue::BigInt(7),
            ],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.rows, 1);
}

#[tokio::test]
async fn profiles_columns() {
    let engine = ready_engine(Duration::from_secs(10)).await;
    let db = engine.open(Uuid::new_v4(), vec![]).await.unwrap();
    let rows = db
        .create_table_as(
            "s_p".into(),
            "t".into(),
            "SELECT i AS id, CASE WHEN i % 4 = 0 THEN NULL ELSE 'r' || (i % 3) END AS region FROM range(1000) t(i)".into(),
        )
        .await
        .unwrap();
    assert_eq!(rows, 1000);
    let (profiles, profiled) = db.profile("\"s_p\".\"t\"".into(), None).await.unwrap();
    assert_eq!(profiled, 1000);
    let region = profiles.iter().find(|p| p.name == "region").unwrap();
    assert!((region.null_fraction.unwrap() - 0.25).abs() < 0.01);
    assert_eq!(region.approx_distinct, Some(3));
    assert!(!region.samples.is_empty());
    let id = profiles.iter().find(|p| p.name == "id").unwrap();
    assert_eq!(
        (id.min.as_deref(), id.max.as_deref()),
        (Some("0"), Some("999"))
    );
}

#[tokio::test]
async fn excel_sheets_become_tables() {
    let engine = ready_engine(Duration::from_secs(10)).await;
    let db = engine.open(Uuid::new_v4(), vec![]).await.unwrap();
    let path = db.uploads_dir().join("FY26 targets.xlsx");
    db.execute(format!(
        "COPY (SELECT 'EMEA' AS region, 3.4 AS target) TO '{}' (FORMAT xlsx, HEADER true, SHEET 'Targets')",
        path.display()
    ))
    .await
    .unwrap();
    let loaded = db
        .load_file(path, FileFormat::Xlsx, "s_x".into())
        .await
        .unwrap();
    assert_eq!(loaded, vec![("targets".to_owned(), 1)]);
}

#[tokio::test]
async fn attaches_sqlite_read_only_and_reopens_after_invalidation() {
    let engine = ready_engine(Duration::from_secs(10)).await;
    let project = Uuid::new_v4();
    let dir = engine.config().project_dir(project).join("uploads");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("shop.sqlite");
    {
        let maker = Connection::open_in_memory().unwrap();
        let ext = engine.config().extension_dir.display().to_string();
        maker
            .execute_batch(&format!(
                "SET extension_directory = '{ext}'; LOAD sqlite;
                 ATTACH '{}' AS s (TYPE sqlite);
                 CREATE TABLE s.items AS SELECT i AS id, 'item ' || i AS name FROM range(25) t(i);",
                file.display()
            ))
            .unwrap();
    }
    let reg = Registration {
        alias: "src_shop".into(),
        statements: attach_sqlite(
            "src_shop",
            &file.canonicalize().unwrap().display().to_string(),
        ),
    };
    let db = engine.open(project, vec![reg.clone()]).await.unwrap();
    assert!(db.attach_errors().is_empty(), "{:?}", db.attach_errors());
    let tables = db.introspect("src_shop".into(), None).await.unwrap();
    assert_eq!(tables[0].name, "items");
    let out = db
        .query(
            "SELECT count(*) FROM \"src_shop\".\"items\"".into(),
            vec![],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.rows, 1);
    let write = db
        .query(
            "INSERT INTO \"src_shop\".\"items\" VALUES (99, 'x')".into(),
            vec![],
            &CancellationToken::new(),
        )
        .await;
    assert!(write.is_err(), "live sources are read-only");

    drop(db);
    engine.invalidate(project).await;
    let reopened = engine.open(project, vec![reg]).await.unwrap();
    assert!(reopened.attach_errors().is_empty());
    assert_eq!(engine.open_projects().await, 1);
}

#[tokio::test]
async fn an_unreachable_source_does_not_block_the_project() {
    let engine = ready_engine(Duration::from_secs(10)).await;
    // SQLite attaches lazily, so use a Postgres that refuses connections (port 1).
    let config = lattice_connectors::DatabaseConfig {
        host: "127.0.0.1".into(),
        port: 1,
        database: "x".into(),
        user: "x".into(),
        schema: None,
    };
    let reg = Registration {
        alias: "src_gone".into(),
        statements: lattice_connectors::attach_database(
            lattice_connectors::SourceKind::Postgres,
            "src_gone",
            &config,
            "x",
        ),
    };
    let db = engine.open(Uuid::new_v4(), vec![reg]).await.unwrap();
    assert_eq!(db.attach_errors().len(), 1);
    assert!(
        db.query("SELECT 1".into(), vec![], &CancellationToken::new())
            .await
            .is_ok()
    );
}
