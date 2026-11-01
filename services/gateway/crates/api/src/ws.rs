//! The browser's data socket.
//!
//! JSON control messages in both directions; query results as binary frames
//! (`[u32 LE header length][ResultHeader JSON][Arrow IPC stream]`). A view sends all its
//! queries in one batch; they run concurrently and each result is sent as soon as it's ready
//! (spec §3 rule 7). A newer batch cancels the older one through to DuckDB (rule 6).

use std::time::Instant;

use axum::extract::State;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::http::header::ORIGIN;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use futures_util::{SinkExt as _, StreamExt as _};
use lattice_security::{Principal, ProjectRole};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::auth::{Authed, SESSION_COOKIE};
use crate::dto::{BatchQuery, ClientMessage, ServerMessage};
use crate::error::ApiError;
use crate::middleware::is_allowed_origin;
use crate::query::{self, QueryFailure};
use crate::state::{AppState, GATEWAY_VERSION};

/// Most queries a single batch may carry (a sheet's worth of visuals, with headroom).
const MAX_QUERIES_PER_BATCH: usize = 64;

pub async fn upgrade(
    State(state): State<AppState>,
    Authed(principal): Authed,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // Browsers always send Origin on WebSocket handshakes, and SameSite doesn't protect
    // them, so this check is what stops cross-site WebSocket hijacking.
    match headers.get(ORIGIN) {
        Some(origin) if is_allowed_origin(origin, &state) => {}
        _ => return Err(ApiError::BadOrigin),
    }
    let token = CookieJar::from_headers(&headers)
        .get(SESSION_COOKIE)
        .map(|c| c.value().to_owned())
        .unwrap_or_default();
    Ok(ws.on_upgrade(move |socket| session(socket, state, principal, token)))
}

enum Outbound {
    Text(String),
    Binary(Vec<u8>),
}

fn text(message: &ServerMessage) -> Outbound {
    // Serialising these enums cannot fail (no maps with non-string keys, no custom impls).
    Outbound::Text(serde_json::to_string(message).unwrap_or_default())
}

async fn session(socket: WebSocket, state: AppState, principal: Principal, token: String) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Outbound>(64);
    let writer = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            let message = match out {
                Outbound::Text(s) => Message::Text(Utf8Bytes::from(s)),
                Outbound::Binary(b) => Message::Binary(b.into()),
            };
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });

    let _ = tx
        .send(text(&ServerMessage::Hello {
            gateway_version: GATEWAY_VERSION.to_owned(),
            user_id: principal.user_id,
        }))
        .await;

    let mut current: Option<(u32, CancellationToken)> = None;
    while let Some(Ok(message)) = stream.next().await {
        let received = Instant::now();
        let Message::Text(raw) = message else {
            if matches!(message, Message::Close(_)) {
                break;
            }
            continue; // protocol pings are answered by the WebSocket layer
        };
        match serde_json::from_str::<ClientMessage>(raw.as_str()) {
            Ok(ClientMessage::Ping { id, client_time_ms }) => {
                let server_us = u32::try_from(received.elapsed().as_micros()).unwrap_or(u32::MAX);
                let _ = tx
                    .send(text(&ServerMessage::Pong {
                        id,
                        client_time_ms,
                        server_us,
                    }))
                    .await;
            }
            Ok(ClientMessage::QueryBatch {
                batch_id,
                project_id,
                queries,
            }) => {
                // The socket outlives page loads: re-check the session and the role on every
                // batch, so signing out or losing access stops the data immediately.
                match state.sessions.lookup(&token).await {
                    Ok(Some(_)) => {}
                    _ => {
                        let _ = tx
                            .send(text(&ServerMessage::Error {
                                code: "session_expired".into(),
                                message: "Your session has ended. Sign in again.".into(),
                            }))
                            .await;
                        break;
                    }
                }
                if let Err((code, message)) =
                    authorise(&state, &principal, project_id, queries.len()).await
                {
                    reject(&tx, batch_id, &queries, code, &message).await;
                    continue;
                }
                if let Some((_, previous)) = current.take() {
                    previous.cancel();
                }
                let cancel = CancellationToken::new();
                current = Some((batch_id, cancel.clone()));
                tokio::spawn(run_batch(
                    state.clone(),
                    tx.clone(),
                    project_id,
                    batch_id,
                    queries,
                    cancel,
                ));
            }
            Ok(ClientMessage::Cancel { batch_id }) => {
                if let Some((id, cancel)) = &current
                    && *id == batch_id
                {
                    cancel.cancel();
                }
            }
            Err(err) => {
                let _ = tx
                    .send(text(&ServerMessage::Error {
                        code: "invalid_message".into(),
                        message: err.to_string(),
                    }))
                    .await;
            }
        }
    }
    // The browser went away: stop any work still running for it.
    if let Some((_, cancel)) = current {
        cancel.cancel();
    }
    drop(tx);
    let _ = writer.await;
    tracing::debug!(user_id = %principal.user_id, "websocket closed");
}

async fn authorise(
    state: &AppState,
    principal: &Principal,
    project_id: Uuid,
    queries: usize,
) -> Result<(), (&'static str, String)> {
    if queries == 0 || queries > MAX_QUERIES_PER_BATCH {
        return Err((
            "invalid_request",
            format!("A batch carries 1–{MAX_QUERIES_PER_BATCH} queries."),
        ));
    }
    match lattice_project::role_of(
        &state.pool,
        principal.workspace_id,
        project_id,
        principal.user_id,
    )
    .await
    {
        Ok(Some(role)) if role >= ProjectRole::Viewer => Ok(()),
        Ok(_) => Err((
            "not_found",
            "That project doesn't exist or you don't have access to it.".into(),
        )),
        Err(err) => Err(("internal", err.to_string())),
    }
}

async fn reject(
    tx: &mpsc::Sender<Outbound>,
    batch_id: u32,
    queries: &[BatchQuery],
    code: &str,
    message: &str,
) {
    for q in queries {
        let _ = tx
            .send(text(&ServerMessage::QueryError {
                batch_id,
                query_id: q.id.clone(),
                code: code.to_owned(),
                message: message.to_owned(),
            }))
            .await;
    }
    let _ = tx
        .send(text(&ServerMessage::BatchDone {
            batch_id,
            total_ms: 0.0,
            cancelled: false,
        }))
        .await;
}

async fn run_batch(
    state: AppState,
    tx: mpsc::Sender<Outbound>,
    project_id: Uuid,
    batch_id: u32,
    queries: Vec<BatchQuery>,
    cancel: CancellationToken,
) {
    let started = Instant::now();
    let mut tasks = JoinSet::new();
    for q in queries {
        let (state, tx, cancel) = (state.clone(), tx.clone(), cancel.clone());
        tasks.spawn(async move {
            match query::run(&state, project_id, batch_id, q.id.clone(), q.query, cancel).await {
                Ok(outcome) => {
                    let _ = tx
                        .send(Outbound::Binary(query::frame(
                            &outcome.header,
                            &outcome.ipc,
                        )))
                        .await;
                }
                // A superseded query's result is unwanted; say nothing.
                Err(QueryFailure::Cancelled) => {}
                Err(QueryFailure::Failed { code, message }) => {
                    let _ = tx
                        .send(text(&ServerMessage::QueryError {
                            batch_id,
                            query_id: q.id,
                            code: code.into(),
                            message,
                        }))
                        .await;
                }
            }
        });
    }
    while tasks.join_next().await.is_some() {}
    let total_ms = (started.elapsed().as_secs_f64() * 1e6).round() / 1e3;
    let _ = tx
        .send(text(&ServerMessage::BatchDone {
            batch_id,
            total_ms,
            cancelled: cancel.is_cancelled(),
        }))
        .await;
}
