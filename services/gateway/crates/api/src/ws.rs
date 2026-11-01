//! The browser's data socket.
//!
//! JSON control messages in both directions; query results as binary frames
//! (`[u32 LE header length][ResultHeader JSON][Arrow IPC stream]`). A view sends all its
//! queries in one batch; they run concurrently and each result is sent as soon as it's ready
//! (spec §3 rule 7). A newer batch cancels the older one through to DuckDB (rule 6).
//!
//! Three batch kinds: `query_batch` (table queries, the explorer), `sheet_batch` (a sheet's
//! visuals with its interactive state) and `shared_batch` (a shared link's visuals). The first
//! two need a signed-in project member; the last is checked against the link (docs/adr/0005),
//! so a socket may also be anonymous.

use std::time::Instant;

use axum::extract::State;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::http::header::ORIGIN;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;
use futures_util::future::join_all;
use futures_util::{SinkExt as _, StreamExt as _};
use lattice_planner::{Scalar, TableQuery};
use lattice_security::{Principal, ProjectRole};
use lattice_visuals::{SheetContext, VisualSpec};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::auth::{MaybeAuthed, SESSION_COOKIE};
use crate::dto::{ClientMessage, ServerMessage};
use crate::error::ApiError;
use crate::middleware::is_allowed_origin;
use crate::query::{self, QueryFailure};
use crate::sheets::{open_link, shared_context};
use crate::state::{AppState, GATEWAY_VERSION};

/// Most queries a single batch may carry (a sheet's worth of visuals, with headroom).
const MAX_QUERIES_PER_BATCH: usize = 64;

pub async fn upgrade(
    State(state): State<AppState>,
    MaybeAuthed(principal): MaybeAuthed,
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

/// One unit of work in a batch.
enum Job {
    Query {
        id: String,
        query: TableQuery,
    },
    Visual {
        id: Uuid,
        spec: VisualSpec,
        drill: Vec<Scalar>,
        offset: Option<u32>,
    },
}

impl Job {
    fn id(&self) -> String {
        match self {
            Job::Query { id, .. } => id.clone(),
            Job::Visual { id, .. } => id.to_string(),
        }
    }
}

/// A batch that passed its checks and is ready to run.
struct Batch {
    project_id: Uuid,
    context: SheetContext,
    jobs: Vec<Job>,
}

async fn session(socket: WebSocket, state: AppState, principal: Option<Principal>, token: String) {
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
            user_id: principal.as_ref().map(|p| p.user_id),
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
        let message = match serde_json::from_str::<ClientMessage>(raw.as_str()) {
            Ok(m) => m,
            Err(err) => {
                let _ = tx
                    .send(text(&ServerMessage::Error {
                        code: "invalid_message".into(),
                        message: err.to_string(),
                    }))
                    .await;
                continue;
            }
        };
        let (batch_id, prepared) = match message {
            ClientMessage::Ping { id, client_time_ms } => {
                let server_us = u32::try_from(received.elapsed().as_micros()).unwrap_or(u32::MAX);
                let _ = tx
                    .send(text(&ServerMessage::Pong {
                        id,
                        client_time_ms,
                        server_us,
                    }))
                    .await;
                continue;
            }
            ClientMessage::Cancel { batch_id } => {
                if let Some((id, cancel)) = &current
                    && *id == batch_id
                {
                    cancel.cancel();
                }
                continue;
            }
            ClientMessage::QueryBatch {
                batch_id,
                project_id,
                queries,
            } => {
                let jobs = queries
                    .into_iter()
                    .map(|q| Job::Query {
                        id: q.id,
                        query: q.query,
                    })
                    .collect();
                let Some(member) = signed_in(&state, principal.as_ref(), &token, &tx).await else {
                    break;
                };
                (
                    batch_id,
                    member_batch(&state, &member, project_id, SheetContext::default(), jobs).await,
                )
            }
            ClientMessage::SheetBatch {
                batch_id,
                project_id,
                context,
                visuals,
            } => {
                let jobs = visuals
                    .into_iter()
                    .map(|v| Job::Visual {
                        id: v.id,
                        spec: v.spec,
                        drill: v.drill,
                        offset: v.offset,
                    })
                    .collect();
                let Some(member) = signed_in(&state, principal.as_ref(), &token, &tx).await else {
                    break;
                };
                (
                    batch_id,
                    member_batch(&state, &member, project_id, context, jobs).await,
                )
            }
            ClientMessage::SharedBatch {
                batch_id,
                token: link,
                context,
                visuals,
            } => {
                // A signed-in viewer whose session has since ended continues anonymously.
                let viewer = match &principal {
                    Some(_) => state.sessions.lookup(&token).await.ok().flatten(),
                    None => None,
                };
                (
                    batch_id,
                    shared_batch(&state, viewer.as_ref(), &link, context, visuals).await,
                )
            }
        };
        match prepared {
            Ok(batch) => {
                if let Some((_, previous)) = current.take() {
                    previous.cancel();
                }
                let cancel = CancellationToken::new();
                current = Some((batch_id, cancel.clone()));
                tokio::spawn(run_batch(
                    state.clone(),
                    tx.clone(),
                    batch_id,
                    batch,
                    cancel,
                ));
            }
            Err((ids, code, message)) => reject(&tx, batch_id, &ids, code, &message).await,
        }
    }
    // The browser went away: stop any work still running for it.
    if let Some((_, cancel)) = current {
        cancel.cancel();
    }
    drop(tx);
    let _ = writer.await;
    tracing::debug!(user_id = ?principal.map(|p| p.user_id), "websocket closed");
}

type Rejection = (Vec<String>, &'static str, String);

/// The socket outlives page loads: re-check the session on every member batch, so signing out
/// or losing access stops the data immediately. None (after telling the browser) ends the socket.
async fn signed_in(
    state: &AppState,
    principal: Option<&Principal>,
    token: &str,
    tx: &mpsc::Sender<Outbound>,
) -> Option<Principal> {
    let message = match principal {
        Some(_) => match state.sessions.lookup(token).await {
            Ok(Some(p)) => return Some(p),
            _ => "Your session has ended. Sign in again.",
        },
        None => "Sign in to query project data.",
    };
    let _ = tx
        .send(text(&ServerMessage::Error {
            code: "session_expired".into(),
            message: message.into(),
        }))
        .await;
    None
}

async fn member_batch(
    state: &AppState,
    principal: &Principal,
    project_id: Uuid,
    context: SheetContext,
    jobs: Vec<Job>,
) -> Result<Batch, Rejection> {
    let ids = || jobs.iter().map(Job::id).collect::<Vec<_>>();
    if jobs.is_empty() || jobs.len() > MAX_QUERIES_PER_BATCH {
        return Err((
            ids(),
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
        Ok(Some(role)) if role >= ProjectRole::Viewer => Ok(Batch {
            project_id,
            context,
            jobs,
        }),
        Ok(_) => Err((
            ids(),
            "not_found",
            "That project doesn't exist or you don't have access to it.".into(),
        )),
        Err(err) => Err((ids(), "internal", err.to_string())),
    }
}

/// Runs only the link's own saved visuals, with the viewer's state narrowed to the sheet's
/// filters and dimensions.
async fn shared_batch(
    state: &AppState,
    viewer: Option<&Principal>,
    link: &str,
    context: SheetContext,
    visuals: Vec<crate::dto::SharedVisualRequest>,
) -> Result<Batch, Rejection> {
    let ids: Vec<String> = visuals.iter().map(|v| v.id.to_string()).collect();
    if visuals.is_empty() || visuals.len() > MAX_QUERIES_PER_BATCH {
        return Err((
            ids,
            "invalid_request",
            format!("A batch carries 1–{MAX_QUERIES_PER_BATCH} visuals."),
        ));
    }
    let shared = match open_link(state, link, viewer).await {
        Ok(shared) => shared,
        Err(ApiError::Unauthenticated) => {
            return Err((ids, "unauthenticated", "Sign in to open this sheet.".into()));
        }
        Err(ApiError::Forbidden) => {
            return Err((ids, "forbidden", "This sheet isn't shared with you.".into()));
        }
        Err(ApiError::NotFound) => {
            return Err((
                ids,
                "not_found",
                "This link doesn't exist or has expired.".into(),
            ));
        }
        Err(err) => return Err((ids, "internal", err.to_string())),
    };
    let context =
        shared_context(&shared.sheet, context).map_err(|m| (ids.clone(), "invalid_request", m))?;
    let mut jobs = Vec::with_capacity(visuals.len());
    for v in visuals {
        let Some(saved) = shared.sheet.visuals.iter().find(|s| s.id == v.id) else {
            return Err((ids, "not_found", "That visual isn't on this sheet.".into()));
        };
        jobs.push(Job::Visual {
            id: v.id,
            spec: saved.spec.clone(),
            drill: v.drill,
            offset: v.offset,
        });
    }
    Ok(Batch {
        project_id: shared.sheet.project_id,
        context,
        jobs,
    })
}

async fn reject(
    tx: &mpsc::Sender<Outbound>,
    batch_id: u32,
    ids: &[String],
    code: &str,
    message: &str,
) {
    for id in ids {
        let _ = tx
            .send(text(&ServerMessage::QueryError {
                batch_id,
                query_id: id.clone(),
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
    batch_id: u32,
    batch: Batch,
    cancel: CancellationToken,
) {
    let started = Instant::now();
    let context = std::sync::Arc::new(batch.context);
    let mut tasks = JoinSet::new();
    for job in batch.jobs {
        let (state, tx, cancel, context) =
            (state.clone(), tx.clone(), cancel.clone(), context.clone());
        let project_id = batch.project_id;
        tasks.spawn(async move {
            match job {
                Job::Query { id, query } => {
                    let result =
                        query::run(&state, project_id, batch_id, id.clone(), query, cancel).await;
                    send(&tx, batch_id, &id, result).await;
                }
                Job::Visual {
                    id,
                    spec,
                    drill,
                    offset,
                } => {
                    run_visual(
                        &state, &tx, project_id, batch_id, id, &spec, &drill, offset, &context,
                        &cancel,
                    )
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

/// Compiles a visual against its table and runs its queries concurrently.
#[allow(clippy::too_many_arguments)]
async fn run_visual(
    state: &AppState,
    tx: &mpsc::Sender<Outbound>,
    project_id: Uuid,
    batch_id: u32,
    id: Uuid,
    spec: &VisualSpec,
    drill: &[Scalar],
    offset: Option<u32>,
    context: &SheetContext,
    cancel: &CancellationToken,
) {
    let started = Instant::now();
    let query_id = id.to_string();
    let table = match query::resolve(state, project_id, spec.table_id).await {
        Ok(t) => t,
        Err(failure) => return send(tx, batch_id, &query_id, Err(failure)).await,
    };
    let parts = match lattice_visuals::compile(id, spec, drill, offset, context, &table.schema) {
        Ok(parts) => parts,
        Err(err) => {
            let failure = QueryFailure::Failed {
                code: "invalid_query",
                message: err.to_string(),
            };
            return send(tx, batch_id, &query_id, Err(failure)).await;
        }
    };
    join_all(parts.into_iter().map(|part| {
        let part_id = format!("{query_id}{}", part.part.suffix());
        let table = &table;
        async move {
            let result = query::execute(
                state,
                project_id,
                batch_id,
                part_id.clone(),
                table,
                &part.query,
                cancel,
                started,
            )
            .await;
            send(tx, batch_id, &part_id, result).await;
        }
    }))
    .await;
}

async fn send(
    tx: &mpsc::Sender<Outbound>,
    batch_id: u32,
    query_id: &str,
    result: Result<query::QueryOutcome, QueryFailure>,
) {
    match result {
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
                    query_id: query_id.to_owned(),
                    code: code.into(),
                    message,
                }))
                .await;
        }
    }
}
