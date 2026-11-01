//! Sources and the WebSocket query protocol, end to end: real Postgres catalog, real DuckDB,
//! real Redis cache, real sockets.

#![allow(clippy::unwrap_used)]

mod common;

use std::io::Cursor;
use std::net::SocketAddr;
use std::str::FromStr as _;
use std::time::{Duration, Instant};

use arrow::ipc::reader::StreamReader;
use axum::http::StatusCode;
use common::*;
use futures_util::{SinkExt as _, StreamExt as _};
use lattice_api::dto::ResultHeader;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use uuid::Uuid;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(addr: SocketAddr, cookie: &str) -> Socket {
    let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    req.headers_mut().insert("origin", WEB.parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let hello = ws.next().await.unwrap().unwrap();
    assert!(hello.to_text().unwrap().contains("\"hello\""));
    ws
}

enum Event {
    Result(ResultHeader, usize),
    Error(Value),
    Done(Value),
}

/// Reads messages until `batch_done` for `batch_id`, returning everything for that batch.
async fn until_done(ws: &mut Socket, batch_id: u32) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        let message = tokio::time::timeout(Duration::from_secs(30), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        match message {
            Message::Binary(bytes) => {
                let (header, ipc) = lattice_api::query::unframe(&bytes).unwrap();
                let rows: usize = StreamReader::try_new(Cursor::new(ipc.to_vec()), None)
                    .unwrap()
                    .map(|b| b.unwrap().num_rows())
                    .sum();
                assert_eq!(rows, header.rows as usize, "header matches the Arrow data");
                if header.batch_id == batch_id {
                    events.push(Event::Result(header, rows));
                }
            }
            Message::Text(text) => {
                let v: Value = serde_json::from_str(text.as_str()).unwrap();
                if v["batch_id"] != json!(batch_id) {
                    continue;
                }
                match v["type"].as_str() {
                    Some("query_error") => events.push(Event::Error(v)),
                    Some("batch_done") => {
                        events.push(Event::Done(v));
                        return events;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

async fn send_batch(ws: &mut Socket, batch_id: u32, project: Uuid, queries: Value) {
    let msg = json!({ "type": "query_batch", "batch_id": batch_id, "project_id": project, "queries": queries });
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();
}

async fn uploaded_table(h: &Harness, cookie: &str, project: Uuid) -> Uuid {
    let reply = upload(
        &h.app,
        cookie,
        project,
        "Orders Q1.csv",
        ORDERS_CSV.as_bytes(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.body["tables"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn uploading_a_csv_creates_a_profiled_extract(pool: PgPool) {
    let h = harness(pool).await;
    let cookie = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &cookie).await;

    let reply = upload(
        &h.app,
        &cookie,
        project,
        "Orders Q1.csv",
        ORDERS_CSV.as_bytes(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    let source = &reply.body["source"];
    assert_eq!(source["kind"], "file");
    assert_eq!(source["status"], "ready");
    assert_eq!(source["row_count"], 5);
    let table = &reply.body["tables"][0];
    assert_eq!(table["name"], "orders_q1");
    assert_eq!(table["mode"], "extract");
    let amount = table["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "amount")
        .unwrap();
    assert!(
        amount["data_type"].as_str().unwrap().starts_with("DECIMAL")
            || amount["data_type"] == "DOUBLE"
    );
    // Max amount in the CSV is 12.00 (DuckDB may type it DOUBLE or DECIMAL).
    let max: f64 = amount["profile"]["max"].as_str().unwrap().parse().unwrap();
    assert!((max - 12.0).abs() < 1e-9, "max {max}");
    let region = table["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "region")
        .unwrap();
    assert_eq!(region["profile"]["approx_distinct"], 3);

    let list = json_call(
        &h.app,
        "GET",
        &format!("/api/projects/{project}/sources"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(list.body["sources"].as_array().unwrap().len(), 1);
    let projects = json_call(&h.app, "GET", "/api/projects", Some(&cookie), None).await;
    assert_eq!(projects.body["projects"][0]["source_count"], 1);
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn uploads_need_admin_rights_and_a_supported_file(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let owner = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &owner).await;

    assert_eq!(
        upload(&h.app, &owner, project, "notes.txt", b"hello")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        upload(&h.app, &owner, project, "empty.csv", b"")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    // Someone else, made a Viewer of the project: can read sources, can't add them.
    let viewer_cookie = signup(&h.app, "vic@acme.example").await;
    let viewer = sqlx::query_scalar!("SELECT id FROM users WHERE email = 'vic@acme.example'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let owner_ws = sqlx::query_scalar!("SELECT workspace_id FROM projects WHERE id = $1", project)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query!(
        "UPDATE sessions SET workspace_id = $1 WHERE user_id = $2",
        owner_ws,
        viewer
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'member')",
        owner_ws,
        viewer
    )
    .execute(&pool)
    .await
    .unwrap();
    lattice_project::add_member(
        &pool,
        project,
        viewer,
        lattice_security::ProjectRole::Viewer,
    )
    .await
    .unwrap();
    assert_eq!(
        upload(
            &h.app,
            &viewer_cookie,
            project,
            "o.csv",
            ORDERS_CSV.as_bytes()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let list = json_call(
        &h.app,
        "GET",
        &format!("/api/projects/{project}/sources"),
        Some(&viewer_cookie),
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK);

    // A stranger in another workspace doesn't learn the project exists.
    let stranger = signup(&h.app, "sam@globex.example").await;
    assert_eq!(
        upload(&h.app, &stranger, project, "o.csv", ORDERS_CSV.as_bytes())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn websocket_batches_stream_arrow_results_and_hit_the_cache(pool: PgPool) {
    let h = harness(pool).await;
    let cookie = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &cookie).await;
    let table = uploaded_table(&h, &cookie, project).await;
    let addr = serve(h.state.clone()).await;
    let mut ws = connect(addr, &cookie).await;

    let queries = json!([
        { "id": "by_region", "query": { "table_id": table, "dimensions": [{ "column": "region" }],
            "measures": [{ "column": "amount", "agg": "sum" }], "sort": [{ "by": "sum_amount", "dir": "desc" }] } },
        { "id": "by_month", "query": { "table_id": table, "dimensions": [{ "column": "order_date", "bucket": "month" }],
            "measures": [{ "agg": "count" }] } },
        { "id": "rows", "query": { "table_id": table, "limit": 2 } }
    ]);
    send_batch(&mut ws, 1, project, queries.clone()).await;
    let events = until_done(&mut ws, 1).await;
    let results: Vec<(String, u32, bool, bool)> = events
        .iter()
        .filter_map(|e| match e {
            Event::Result(h, _) => Some((h.query_id.clone(), h.rows, h.truncated, h.cached)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 3, "every query answered");
    assert!(results.contains(&("by_region".into(), 3, false, false)));
    assert!(results.contains(&("by_month".into(), 3, false, false)));
    assert!(
        results.contains(&("rows".into(), 2, true, false)),
        "limit 2 of 5 rows → truncated"
    );
    if let Some(Event::Result(header, _)) = events
        .iter()
        .find(|e| matches!(e, Event::Result(h, _) if h.query_id == "by_region"))
    {
        assert!(header.timings.total_ms > 0.0 && header.timings.execute_ms > 0.0);
    }

    // The cache is filled off the response path; give it a moment, then ask again.
    tokio::time::sleep(Duration::from_millis(200)).await;
    send_batch(&mut ws, 2, project, queries).await;
    let cached = until_done(&mut ws, 2).await;
    let all_cached = cached
        .iter()
        .filter(|e| matches!(e, Event::Result(h, _) if h.cached))
        .count();
    assert_eq!(all_cached, 3, "second identical batch is served from Redis");
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn websocket_queries_are_scoped_to_the_project(pool: PgPool) {
    let h = harness(pool).await;
    let ana = signup(&h.app, "ana@acme.example").await;
    let anas_project = create_project(&h.app, &ana).await;
    let anas_table = uploaded_table(&h, &ana, anas_project).await;

    let bob = signup(&h.app, "bob@globex.example").await;
    let bobs_project = create_project(&h.app, &bob).await;
    let addr = serve(h.state.clone()).await;
    let mut ws = connect(addr, &bob).await;

    // Bob's own project, Ana's table id: the table doesn't exist for him.
    send_batch(&mut ws, 1, bobs_project, json!([{ "id": "q", "query": { "table_id": anas_table, "measures": [{ "agg": "count" }] } }])).await;
    let events = until_done(&mut ws, 1).await;
    assert!(matches!(&events[0], Event::Error(v) if v["code"] == "not_found"));

    // Ana's project id: Bob isn't a member.
    send_batch(&mut ws, 2, anas_project, json!([{ "id": "q", "query": { "table_id": anas_table, "measures": [{ "agg": "count" }] } }])).await;
    let events = until_done(&mut ws, 2).await;
    assert!(matches!(&events[0], Event::Error(v) if v["code"] == "not_found"));

    // Invalid queries are rejected by the compiler before reaching DuckDB.
    let mut ws_ana = connect(addr, &ana).await;
    send_batch(&mut ws_ana, 3, anas_project, json!([{ "id": "q", "query": { "table_id": anas_table, "measures": [{ "column": "region", "agg": "sum" }] } }])).await;
    let events = until_done(&mut ws_ana, 3).await;
    assert!(matches!(&events[0], Event::Error(v) if v["code"] == "invalid_query"));
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn a_newer_batch_cancels_the_older_one(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let demo = lattice_api::seed::seed_demo(&pool).await.unwrap().unwrap();
    lattice_api::seed::seed_demo_data(&h.state, &demo, 5_000_000)
        .await
        .unwrap();
    let orders: Uuid =
        sqlx::query_scalar!("SELECT id FROM source_tables WHERE table_name = 'orders'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let login = json_call(
        &h.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": lattice_api::seed::DEMO_EMAIL, "password": lattice_api::seed::DEMO_PASSWORD })),
    )
    .await;
    let cookie = login.cookie.unwrap();
    let addr = serve(h.state.clone()).await;
    let mut ws = connect(addr, &cookie).await;

    // 5M groups: slow enough to still be running when the next batch arrives.
    send_batch(&mut ws, 1, demo.revenue_project, json!([{ "id": "slow", "query": { "table_id": orders,
        "dimensions": [{ "column": "order_id" }], "measures": [{ "column": "amount", "agg": "sum" }], "limit": 10 } }])).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let started = Instant::now();
    send_batch(
        &mut ws,
        2,
        demo.revenue_project,
        json!([{ "id": "fast", "query": { "table_id": orders,
        "dimensions": [{ "column": "region" }], "measures": [{ "agg": "count" }] } }]),
    )
    .await;

    let first = until_done(&mut ws, 1).await;
    assert!(
        matches!(first.last(), Some(Event::Done(v)) if v["cancelled"] == true),
        "older batch was cancelled"
    );
    assert!(
        !first.iter().any(|e| matches!(e, Event::Result(..))),
        "and sent no stale result"
    );
    let second = until_done(&mut ws, 2).await;
    assert!(matches!(&second[0], Event::Result(h, 3) if h.query_id == "fast"));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn postgres_sources_attach_live_and_answer_queries(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let cookie = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &cookie).await;

    // The test's own database stands in for a customer's Postgres.
    let server = PgConnectOptions::from_str(&std::env::var("DATABASE_URL").unwrap()).unwrap();
    let password = std::env::var("DATABASE_URL")
        .unwrap()
        .split(':')
        .nth(2)
        .unwrap()
        .split('@')
        .next()
        .unwrap()
        .to_owned();
    let database = pool.connect_options().get_database().unwrap().to_owned();
    let request = json!({
        "kind": "postgres",
        "name": "Ops database",
        "config": { "host": server.get_host(), "port": server.get_port(), "database": database,
                    "user": server.get_username(), "schema": "public" },
        "password": password,
    });
    let started = Instant::now();
    let reply = json_call(
        &h.app,
        "POST",
        &format!("/api/projects/{project}/sources"),
        Some(&cookie),
        Some(request.clone()),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    println!(
        "connected + introspected a live Postgres in {:?}",
        started.elapsed()
    );
    let tables = reply.body["tables"].as_array().unwrap();
    let users = tables
        .iter()
        .find(|t| t["name"] == "users")
        .expect("introspected the users table");
    assert_eq!(users["mode"], "live");

    // Stored credentials are encrypted, never plaintext.
    let secret: Vec<u8> = sqlx::query_scalar!("SELECT secret FROM sources WHERE kind = 'postgres'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !secret
            .windows(password.len())
            .any(|w| w == password.as_bytes())
    );

    // Query the live table through the WebSocket.
    let addr = serve(h.state.clone()).await;
    let mut ws = connect(addr, &cookie).await;
    send_batch(&mut ws, 1, project, json!([{ "id": "n", "query": { "table_id": users["id"], "measures": [{ "agg": "count" }] } }])).await;
    let events = until_done(&mut ws, 1).await;
    assert!(
        matches!(&events[0], Event::Result(h, 1) if !h.cached),
        "live count answered"
    );

    // Wrong credentials and the gateway's own metadata database are refused, nothing saved.
    let mut bad = request.clone();
    bad["password"] = json!("wrong-password");
    let reply = json_call(
        &h.app,
        "POST",
        &format!("/api/projects/{project}/sources"),
        Some(&cookie),
        Some(bad),
    )
    .await;
    assert_eq!(
        reply.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{:?}",
        reply.body
    );
    let mut unreachable = request;
    unreachable["config"]["port"] = json!(1);
    let reply = json_call(
        &h.app,
        "POST",
        &format!("/api/projects/{project}/sources"),
        Some(&cookie),
        Some(unreachable),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let count: i64 = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM sources"#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn introspecting_500_tables_stays_within_budget(pool: PgPool) {
    // Spec §3: schema introspection of a 500-table source < 5 s.
    let h = harness(pool).await;
    let cookie = signup(&h.app, "ana@acme.example").await;
    let project = create_project(&h.app, &cookie).await;
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sqlite-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("wide.sqlite");
    {
        let maker = duckdb::Connection::open_in_memory().unwrap();
        let ext = h.state.engine.config().extension_dir.display().to_string();
        let mut sql = format!(
            "SET extension_directory = '{ext}'; LOAD sqlite; ATTACH '{}' AS s (TYPE sqlite);",
            file.display()
        );
        for i in 0..500 {
            sql.push_str(&format!(
                "CREATE TABLE s.t{i:03} (id INTEGER, name VARCHAR, amount DOUBLE, created DATE);"
            ));
        }
        maker.execute_batch(&sql).unwrap();
    }
    let bytes = std::fs::read(&file).unwrap();
    let started = Instant::now();
    let reply = upload(&h.app, &cookie, project, "wide.sqlite", &bytes).await;
    let elapsed = started.elapsed();
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    assert_eq!(reply.body["tables"].as_array().unwrap().len(), 500);
    println!("uploaded + introspected 500 SQLite tables (+ profiled the first 50) in {elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}
