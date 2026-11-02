//! HTTP and WebSocket surface of the gateway: routing, sessions, RBAC checks and the
//! per-request timing breakdown.

pub mod auth;
pub mod capture;
pub mod config;
pub mod db;
pub mod dto;
pub mod error;
pub mod middleware;
pub mod query;
pub mod review;
pub mod routes;
pub mod seed;
pub mod semantic;
pub mod sheets;
pub mod sources;
pub mod state;
pub mod ws;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::routing::{get, post};
use tower::ServiceBuilder;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate as _, SizeAbove};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub use config::Config;
pub use state::{AppState, GATEWAY_VERSION, StartupError};

use crate::auth::{oidc, password};
use crate::routes::{
    health, knowledge as knowledge_routes, model as model_routes, projects, sheets as sheet_routes,
    sources as source_routes,
};

pub fn router(state: AppState) -> Router {
    // Brute-force brake on credential endpoints: bursts of 20, then one attempt per second per IP.
    let auth_limit = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(1)
            .burst_size(20)
            .finish()
            .unwrap_or_default(),
    );
    let limiter = auth_limit.limiter().clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            limiter.retain_recent();
        }
    });

    let credentials = Router::new()
        .route("/auth/signup", post(password::signup))
        .route("/auth/login", post(password::login))
        .layer(GovernorLayer::new(auth_limit));

    let api = Router::new()
        .merge(credentials)
        .route("/auth/providers", get(auth::providers))
        .route("/auth/me", get(auth::me))
        .route("/auth/logout", post(password::logout))
        .route("/auth/oidc/{provider}/start", get(oidc::start))
        .route("/auth/oidc/{provider}/callback", get(oidc::callback))
        .route("/projects", get(projects::list).post(projects::create))
        .route("/projects/{id}", get(projects::get))
        .route("/projects/{id}/members", post(projects::add_member))
        .route(
            "/projects/{id}/members/{user_id}",
            axum::routing::patch(projects::update_member).delete(projects::remove_member),
        )
        .route(
            "/projects/{id}/sources",
            get(source_routes::list).post(source_routes::create_database),
        )
        .route(
            "/projects/{id}/sources/{source_id}",
            get(source_routes::get).delete(source_routes::delete),
        )
        .route(
            "/projects/{id}/sources/{source_id}/refresh",
            post(source_routes::refresh),
        )
        .route(
            "/projects/{id}/tables/{table_id}/profile",
            post(source_routes::profile),
        )
        .route(
            "/projects/{id}/sheets",
            get(sheet_routes::list).post(sheet_routes::create),
        )
        .route("/projects/{id}/sheets/order", post(sheet_routes::reorder))
        .route(
            "/projects/{id}/sheets/{sheet_id}",
            get(sheet_routes::get)
                .put(sheet_routes::save)
                .delete(sheet_routes::delete),
        )
        .route(
            "/projects/{id}/sheets/{sheet_id}/sharing",
            get(sheet_routes::get_sharing).put(sheet_routes::set_sharing),
        )
        .route(
            "/projects/{id}/sheets/{sheet_id}/members",
            post(sheet_routes::add_member),
        )
        .route(
            "/projects/{id}/sheets/{sheet_id}/members/{user_id}",
            axum::routing::delete(sheet_routes::remove_member),
        )
        .route("/shared/{token}", get(sheet_routes::shared))
        .route(
            "/workspace/members",
            get(knowledge_routes::workspace_members),
        )
        .route("/projects/{id}/model", get(model_routes::get))
        .route(
            "/projects/{id}/bindings/{note_id}",
            axum::routing::put(model_routes::bind).delete(model_routes::unbind),
        )
        .route("/knowledge/graph", get(knowledge_routes::graph))
        .route("/knowledge/queue", get(knowledge_routes::queue))
        .route("/knowledge/suggestions", get(knowledge_routes::suggestions))
        .route(
            "/projects/{id}/suggestions/accept",
            post(knowledge_routes::accept_suggestion),
        )
        .route(
            "/projects/{id}/suggestions/dismiss",
            post(knowledge_routes::dismiss_suggestion),
        )
        .route("/knowledge/notes", post(knowledge_routes::create))
        .route("/knowledge/notes/{id}", get(knowledge_routes::get))
        .route(
            "/knowledge/notes/{id}/cases",
            post(knowledge_routes::run_cases),
        )
        .route(
            "/knowledge/notes/{id}/impact",
            get(knowledge_routes::impact),
        )
        .route(
            "/knowledge/notes/{id}/draft",
            axum::routing::put(knowledge_routes::save_draft),
        )
        .route(
            "/knowledge/notes/{id}/submit",
            post(knowledge_routes::submit),
        )
        .route(
            "/knowledge/notes/{id}/approve",
            post(knowledge_routes::approve),
        )
        .route(
            "/knowledge/notes/{id}/reject",
            post(knowledge_routes::reject),
        )
        .route(
            "/knowledge/notes/{id}/discard",
            post(knowledge_routes::discard),
        )
        .route(
            "/knowledge/notes/{id}/revert",
            post(knowledge_routes::revert),
        )
        .route(
            "/knowledge/notes/{id}/deprecate",
            post(knowledge_routes::deprecate),
        )
        .route(
            "/knowledge/notes/{id}/owner",
            axum::routing::put(knowledge_routes::set_owner),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ));

    // Uploads stream up to 200 MB and load them into DuckDB: their own size and time limits.
    let uploads = Router::new()
        .route("/projects/{id}/sources/upload", post(source_routes::upload))
        .layer(DefaultBodyLimit::max(
            usize::try_from(lattice_connectors::MAX_UPLOAD_BYTES).unwrap_or(usize::MAX)
                + 1024 * 1024,
        ))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(600),
        ));

    let web_origin = HeaderValue::from_str(&state.config.web_origin)
        .unwrap_or_else(|_| HeaderValue::from_static("http://localhost:3000"));
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(web_origin.clone()))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([header::CONTENT_TYPE])
        .expose_headers([HeaderName::from_static("server-timing")])
        // Chrome caps this at 2 h; it saves a preflight round trip on every JSON POST.
        .max_age(Duration::from_secs(7200));

    // Compressing sub-kilobyte JSON costs more CPU than it saves on the wire.
    let compression =
        CompressionLayer::new().compress_when(SizeAbove::new(1024).and(NotForContentType::GRPC));

    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/ws", get(ws::upgrade))
        .nest("/api", api.merge(uploads))
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(TraceLayer::new_for_http())
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(cors)
                // Lets the web app read Server-Timing via the Resource Timing API cross-origin.
                .layer(SetResponseHeaderLayer::overriding(
                    HeaderName::from_static("timing-allow-origin"),
                    web_origin,
                ))
                .layer(compression)
                .layer(axum::middleware::from_fn(middleware::server_timing))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    middleware::origin_guard,
                )),
        )
        .with_state(state)
}
