//! Shared harness: a real router over a real Postgres (from `#[sqlx::test]`), a real DuckDB
//! engine in a temporary data directory, and Redis from REDIS_URL (default localhost).

#![allow(clippy::unwrap_used, dead_code)]

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use axum::{Extension, Router as AxumRouter};
use http_body_util::BodyExt as _;
use lattice_api::{AppState, Config};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt as _;
use uuid::Uuid;

pub const WEB: &str = "http://localhost:3000";
pub const SECRET_KEY: &str = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

pub struct Harness {
    pub app: Router,
    pub state: AppState,
}

pub async fn harness(pool: PgPool) -> Harness {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let data_dir = tmp.join(format!("api-test-{}", Uuid::new_v4()));
    let ext_dir = tmp.join("duckdb-extensions");
    let redis = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let config = Config::from_lookup(|key| match key {
        "DATABASE_URL" => Some("postgres://unused".into()),
        "REDIS_URL" => Some(redis.clone()),
        "AI_GRPC_URL" => Some("http://127.0.0.1:1".into()),
        "WEB_ORIGIN" => Some(WEB.into()),
        "DATA_DIR" => Some(data_dir.display().to_string()),
        "DUCKDB_EXTENSION_DIR" => Some(ext_dir.display().to_string()),
        "LATTICE_SECRET_KEY" => Some(SECRET_KEY.into()),
        "ENGINE_THREADS" => Some("4".into()),
        "ENGINE_MEMORY_LIMIT" => Some("1GB".into()),
        _ => None,
    })
    .unwrap();
    let state = AppState::new(config, pool).unwrap();
    state.engine.install_extensions().await.unwrap();
    let app = lattice_api::router(state.clone()).layer(Extension(ConnectInfo(SocketAddr::from((
        [127, 0, 0, 1],
        40000,
    )))));
    Harness { app, state }
}

/// Serves the router on an ephemeral port (WebSocket tests need real sockets).
pub async fn serve(state: AppState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app: AxumRouter = lattice_api::router(state);
    tokio::spawn(
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );
    addr
}

pub struct Reply {
    pub status: StatusCode,
    pub cookie: Option<String>,
    pub body: Value,
}

pub async fn send(app: &Router, req: Request<Body>) -> Reply {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let cookie = res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.starts_with("lattice_session=") && !c.starts_with("lattice_session=;"))
        .map(|c| c.split(';').next().unwrap().to_owned());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        cookie,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

pub async fn json_call(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> Reply {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::ORIGIN, WEB);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    send(app, req).await
}

pub async fn signup(app: &Router, email: &str) -> String {
    let reply = json_call(
        app,
        "POST",
        "/api/auth/signup",
        None,
        Some(json!({ "email": email, "password": "correct-horse-battery", "display_name": "Test User", "workspace_name": "Acme" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.body);
    reply.cookie.unwrap()
}

pub async fn create_project(app: &Router, cookie: &str) -> Uuid {
    let reply = json_call(
        app,
        "POST",
        "/api/projects",
        Some(cookie),
        Some(json!({ "name": "Revenue" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED);
    reply.body["id"].as_str().unwrap().parse().unwrap()
}

/// `multipart/form-data` upload of one file in the `file` field.
pub async fn upload(
    app: &Router,
    cookie: &str,
    project: Uuid,
    file_name: &str,
    content: &[u8],
) -> Reply {
    let boundary = "lattice-test-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/projects/{project}/sources/upload"))
        .header(header::ORIGIN, WEB)
        .header(header::COOKIE, cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    send(app, req).await
}

pub const ORDERS_CSV: &str = "order_date,region,category,amount\n\
2026-01-05,EMEA,Outdoor,10.50\n2026-01-20,EMEA,Apparel,4.50\n2026-02-03,AMER,Outdoor,7.00\n\
2026-02-10,APAC,Apparel,3.25\n2026-03-01,AMER,Outdoor,12.00\n";
