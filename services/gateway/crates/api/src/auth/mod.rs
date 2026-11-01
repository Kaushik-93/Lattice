pub mod oidc;
pub mod password;

use std::time::Instant;

use axum::Json;
use axum::extract::{FromRequestParts, State};
use axum::http::HeaderMap;
use axum::http::header::USER_AGENT;
use axum::http::request::Parts;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::TimeDelta;
use lattice_security::Principal;
use lattice_security::identity::Account;
use lattice_telemetry::ServerTiming;

use crate::dto::{AuthProvidersResponse, MeResponse};
use crate::error::ApiError;
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "lattice_session";

/// "Keep me signed in for 30 days".
const REMEMBERED_SESSION: TimeDelta = TimeDelta::days(30);
/// Otherwise the cookie dies with the browser session, and the server forgets it after 12 h.
const BROWSER_SESSION: TimeDelta = TimeDelta::hours(12);

/// Extractor for handlers that require a signed-in caller. Resolving it costs one indexed
/// query, recorded as the `auth` phase of the Server-Timing header.
pub struct Authed(pub Principal);

impl FromRequestParts<AppState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = CookieJar::from_headers(&parts.headers)
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_owned())
            .ok_or(ApiError::Unauthenticated)?;
        let started = Instant::now();
        let principal = state.sessions.lookup(&token).await?;
        if let Some(timing) = parts.extensions.get::<ServerTiming>() {
            timing.record("auth", started.elapsed());
        }
        principal.map(Authed).ok_or(ApiError::Unauthenticated)
    }
}

/// Extractor for handlers open to anonymous callers that behave differently when signed in
/// (shared sheet links). Never rejects for a missing or stale session.
pub struct MaybeAuthed(pub Option<Principal>);

impl FromRequestParts<AppState> for MaybeAuthed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let Some(token) = CookieJar::from_headers(&parts.headers)
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_owned())
        else {
            return Ok(MaybeAuthed(None));
        };
        let started = Instant::now();
        let principal = state.sessions.lookup(&token).await?;
        if let Some(timing) = parts.extensions.get::<ServerTiming>() {
            timing.record("auth", started.elapsed());
        }
        Ok(MaybeAuthed(principal))
    }
}

/// Creates a session for `account` and returns the jar with the session cookie set.
pub(crate) async fn start_session(
    state: &AppState,
    jar: CookieJar,
    account: Account,
    remember: bool,
    headers: &HeaderMap,
) -> Result<(CookieJar, String), ApiError> {
    let ttl = if remember {
        REMEMBERED_SESSION
    } else {
        BROWSER_SESSION
    };
    let user_agent = headers.get(USER_AGENT).and_then(|v| v.to_str().ok());
    let (token, _expires_at) = state
        .sessions
        .create(account.user_id, account.workspace_id, ttl, user_agent)
        .await?;

    let mut cookie = Cookie::build((SESSION_COOKIE, token.as_str().to_owned()))
        .path("/")
        .http_only(true)
        .secure(state.config.cookie_secure)
        .same_site(SameSite::Lax);
    if remember {
        cookie = cookie.max_age(cookie::time::Duration::seconds(ttl.num_seconds()));
    }
    Ok((jar.add(cookie.build()), token.as_str().to_owned()))
}

pub(crate) fn clear_session_cookie(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::build(SESSION_COOKIE).path("/").build())
}

pub async fn me(Authed(principal): Authed) -> Json<MeResponse> {
    Json(principal.into())
}

pub async fn providers(State(state): State<AppState>) -> Json<AuthProvidersResponse> {
    Json(AuthProvidersResponse {
        providers: state.oidc.list(),
        password: true,
    })
}
