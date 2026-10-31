//! End-to-end tests of the HTTP surface: real router, real Postgres (each test gets a fresh
//! migrated database from `#[sqlx::test]`; set DATABASE_URL to a server you can create
//! databases on). Redis and the AI service connect lazily and aren't needed here.

// Helpers live outside #[test] fns, which clippy's allow-unwrap-in-tests doesn't cover.
#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;

use axum::Extension;
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use lattice_api::{AppState, Config};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt as _;

const WEB: &str = "http://localhost:3000";

fn app(pool: PgPool) -> Router {
    let config = Config::from_lookup(|key| match key {
        "DATABASE_URL" => Some("postgres://unused".into()),
        "REDIS_URL" => Some("redis://127.0.0.1:1".into()),
        "AI_GRPC_URL" => Some("http://127.0.0.1:1".into()),
        "WEB_ORIGIN" => Some(WEB.into()),
        "COOKIE_SECURE" => Some("true".into()),
        _ => None,
    })
    .unwrap();
    lattice_api::router(AppState::new(config, pool).unwrap())
        // A real ConnectInfo, not MockConnectInfo: the rate limiter reads the raw extension.
        .layer(Extension(ConnectInfo(SocketAddr::from((
            [127, 0, 0, 1],
            40000,
        )))))
}

struct Reply {
    status: StatusCode,
    cookie: Option<String>,
    set_cookie: Vec<String>,
    server_timing: Option<String>,
    body: Value,
    /// Raw response text, for assertion messages when the body isn't JSON.
    raw: String,
}

async fn call(
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
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, cookie);
    }
    let req = match body {
        Some(body) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    let set_cookie: Vec<String> = res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    let cookie = set_cookie
        .iter()
        .find(|c| c.starts_with("lattice_session=") && !c.starts_with("lattice_session=;"))
        .map(|c| c.split(';').next().unwrap().to_owned());
    let server_timing = res
        .headers()
        .get("server-timing")
        .map(|v| v.to_str().unwrap().to_owned());
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    Reply {
        status,
        cookie,
        set_cookie,
        server_timing,
        body,
        raw,
    }
}

async fn signup(app: &Router, email: &str, workspace: &str) -> String {
    let reply = call(
        app,
        "POST",
        "/api/auth/signup",
        None,
        Some(json!({
            "email": email, "password": "correct-horse-battery", "display_name": "Ana Lopez",
            "workspace_name": workspace, "remember": true
        })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.raw);
    reply.cookie.expect("signup sets a session cookie")
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn signup_sets_a_hardened_cookie_and_returns_me(pool: PgPool) {
    let app = app(pool);
    let reply = call(
        &app,
        "POST",
        "/api/auth/signup",
        None,
        Some(json!({
            "email": " Ana@Acme.example ", "password": "correct-horse-battery",
            "display_name": "Ana Lopez", "workspace_name": "Acme Retail", "remember": true
        })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED);
    let cookie = reply
        .set_cookie
        .iter()
        .find(|c| c.starts_with("lattice_session="))
        .unwrap();
    for attribute in [
        "HttpOnly",
        "Secure",
        "SameSite=Lax",
        "Path=/",
        "Max-Age=2592000",
    ] {
        assert!(
            cookie.contains(attribute),
            "missing {attribute} in {cookie}"
        );
    }
    assert_eq!(reply.body["user"]["email"], "ana@acme.example");
    assert_eq!(reply.body["user"]["initials"], "AL");
    assert_eq!(reply.body["workspace"]["slug"], "acme-retail");
    assert_eq!(reply.body["workspace_role"], "admin");

    let me = call(&app, "GET", "/api/auth/me", reply.cookie.as_deref(), None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["workspace"]["name"], "Acme Retail");
    let timing = me.server_timing.unwrap();
    assert!(
        timing.contains("auth;dur=") && timing.contains("total;dur="),
        "{timing}"
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn duplicate_email_is_a_conflict(pool: PgPool) {
    let app = app(pool);
    signup(&app, "ana@acme.example", "Acme").await;
    let reply = call(
        &app,
        "POST",
        "/api/auth/signup",
        None,
        Some(json!({
            "email": "ANA@acme.example", "password": "another-password-1",
            "display_name": "Imposter", "workspace_name": "Other"
        })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.body["error"]["code"], "conflict");
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn weak_input_is_rejected_with_a_readable_message(pool: PgPool) {
    let app = app(pool);
    let reply = call(
        &app,
        "POST",
        "/api/auth/signup",
        None,
        Some(json!({ "email": "ana@acme.example", "password": "short", "display_name": "A", "workspace_name": "W" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert!(
        reply.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at least 10")
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn login_logout_cycle(pool: PgPool) {
    let app = app(pool);
    signup(&app, "ana@acme.example", "Acme").await;

    for (email, password) in [
        ("ana@acme.example", "wrong-password-x"),
        ("nobody@acme.example", "whatever-123"),
    ] {
        let reply = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            Some(json!({ "email": email, "password": password })),
        )
        .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        // Same code for unknown user and wrong password: no account-existence oracle.
        assert_eq!(reply.body["error"]["code"], "invalid_credentials");
    }

    let login = call(
        &app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": "ANA@acme.example", "password": "correct-horse-battery", "remember": false })),
    )
    .await;
    assert_eq!(login.status, StatusCode::OK);
    let session_cookie = login
        .set_cookie
        .iter()
        .find(|c| c.starts_with("lattice_session="))
        .unwrap();
    assert!(
        !session_cookie.contains("Max-Age"),
        "browser-session cookie must not persist: {session_cookie}"
    );
    let cookie = login.cookie.unwrap();

    assert_eq!(
        call(&app, "GET", "/api/auth/me", Some(&cookie), None)
            .await
            .status,
        StatusCode::OK
    );
    let logout = call(&app, "POST", "/api/auth/logout", Some(&cookie), None).await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert!(
        logout
            .set_cookie
            .iter()
            .any(|c| c.starts_with("lattice_session=;") || c.contains("Max-Age=0"))
    );
    // The token is revoked server-side, not just dropped by the browser.
    assert_eq!(
        call(&app, "GET", "/api/auth/me", Some(&cookie), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn unauthenticated_and_forged_sessions_are_rejected(pool: PgPool) {
    let app = app(pool);
    assert_eq!(
        call(&app, "GET", "/api/projects", None, None).await.status,
        StatusCode::UNAUTHORIZED
    );
    let forged = format!("lattice_session={}", "A".repeat(43));
    assert_eq!(
        call(&app, "GET", "/api/projects", Some(&forged), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn projects_create_list_and_detail(pool: PgPool) {
    let app = app(pool);
    let cookie = signup(&app, "ana@acme.example", "Acme").await;

    let empty = call(&app, "GET", "/api/projects", Some(&cookie), None).await;
    assert_eq!(empty.body["projects"], json!([]));

    let created = call(
        &app,
        "POST",
        "/api/projects",
        Some(&cookie),
        Some(json!({ "name": "  Revenue Analytics ", "description": "Net revenue for Finance." })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["name"], "Revenue Analytics");
    assert_eq!(created.body["my_role"], "admin");
    assert_eq!(created.body["owned_by_me"], true);
    assert_eq!(created.body["member_initials"], json!(["AL"]));
    assert!(created.body["color"].as_str().unwrap().starts_with('#'));
    let id = created.body["id"].as_str().unwrap().to_owned();

    let list = call(&app, "GET", "/api/projects", Some(&cookie), None).await;
    assert_eq!(list.body["projects"].as_array().unwrap().len(), 1);
    assert!(list.server_timing.unwrap().contains("db;dur="));

    let detail = call(
        &app,
        "GET",
        &format!("/api/projects/{id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["members"][0]["is_owner"], true);
    assert_eq!(detail.body["members"][0]["role"], "admin");
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn workspaces_are_isolated(pool: PgPool) {
    let app = app(pool);
    let ana = signup(&app, "ana@acme.example", "Acme").await;
    let bob = signup(&app, "bob@globex.example", "Globex").await;

    let created = call(
        &app,
        "POST",
        "/api/projects",
        Some(&ana),
        Some(json!({ "name": "Secret plan" })),
    )
    .await;
    let id = created.body["id"].as_str().unwrap();

    let peek = call(
        &app,
        "GET",
        &format!("/api/projects/{id}"),
        Some(&bob),
        None,
    )
    .await;
    assert_eq!(
        peek.status,
        StatusCode::NOT_FOUND,
        "other tenants must not even learn the project exists"
    );
    let bobs = call(&app, "GET", "/api/projects", Some(&bob), None).await;
    assert_eq!(bobs.body["projects"], json!([]));
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn cross_site_writes_are_refused(pool: PgPool) {
    let app = app(pool);
    let cookie = signup(&app, "ana@acme.example", "Acme").await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/projects")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "name": "pwned" }).to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn demo_seed_is_idempotent_and_signs_in(pool: PgPool) {
    assert!(lattice_api::seed::seed_demo(&pool).await.unwrap());
    assert!(!lattice_api::seed::seed_demo(&pool).await.unwrap());

    let app = app(pool);
    let login = call(
        &app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": lattice_api::seed::DEMO_EMAIL, "password": lattice_api::seed::DEMO_PASSWORD })),
    )
    .await;
    assert_eq!(login.status, StatusCode::OK);
    let projects = call(&app, "GET", "/api/projects", login.cookie.as_deref(), None).await;
    let names: Vec<&str> = projects.body["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"Revenue Analytics"));
}

#[sqlx::test(migrator = "lattice_api::db::MIGRATOR")]
async fn liveness_needs_no_dependencies(pool: PgPool) {
    let app = app(pool);
    let req = Request::builder()
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().contains_key("server-timing"));
}
