use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::Response;
use lattice_telemetry::ServerTiming;

use crate::error::ApiError;
use crate::state::AppState;

/// Gives each request a [`ServerTiming`] (handlers record phases into it) and returns the
/// breakdown as a `Server-Timing` header.
pub async fn server_timing(mut request: Request, next: Next) -> Response {
    let started = Instant::now();
    let timing = ServerTiming::new();
    request.extensions_mut().insert(timing.clone());
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&timing.header_value(started.elapsed())) {
        response.headers_mut().insert("server-timing", value);
    }
    response
}

/// CSRF defence in depth on top of SameSite=Lax cookies: a state-changing request that
/// carries an `Origin` must come from the web app (or the gateway itself). Requests with
/// no `Origin` are non-browser clients, which cannot ride on a victim's cookies.
pub async fn origin_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let safe_method = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe_method
        && let Some(origin) = request.headers().get(header::ORIGIN)
        && !is_allowed_origin(origin, &state)
    {
        return Err(ApiError::BadOrigin);
    }
    Ok(next.run(request).await)
}

pub fn is_allowed_origin(origin: &HeaderValue, state: &AppState) -> bool {
    let origin = origin.as_bytes();
    origin == state.config.web_origin.as_bytes() || origin == state.config.public_url.as_bytes()
}
