//! The browser's data socket. Phase 0 carries JSON control messages (hello, ping/pong for
//! the perf overlay's RTT readout); phase 1 adds per-sheet query batches answered with
//! Arrow IPC binary frames.

use std::time::Instant;

use axum::extract::State;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::http::header::ORIGIN;
use axum::response::Response;
use lattice_security::Principal;

use crate::auth::Authed;
use crate::dto::{ClientMessage, ServerMessage};
use crate::error::ApiError;
use crate::middleware::is_allowed_origin;
use crate::state::{AppState, GATEWAY_VERSION};

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
    Ok(ws.on_upgrade(move |socket| session(socket, principal)))
}

async fn session(mut socket: WebSocket, principal: Principal) {
    let hello = ServerMessage::Hello {
        gateway_version: GATEWAY_VERSION.to_owned(),
        user_id: principal.user_id,
    };
    if send(&mut socket, &hello).await.is_err() {
        return;
    }

    while let Some(Ok(message)) = socket.recv().await {
        let received = Instant::now();
        let reply = match message {
            Message::Text(text) => match serde_json::from_str::<ClientMessage>(text.as_str()) {
                Ok(ClientMessage::Ping { id, client_time_ms }) => ServerMessage::Pong {
                    id,
                    client_time_ms,
                    server_us: u32::try_from(received.elapsed().as_micros()).unwrap_or(u32::MAX),
                },
                Err(err) => ServerMessage::Error {
                    code: "invalid_message".into(),
                    message: err.to_string(),
                },
            },
            Message::Close(_) => break,
            // Pings are answered by the protocol layer; binary frames are phase 1.
            _ => continue,
        };
        if send(&mut socket, &reply).await.is_err() {
            break;
        }
    }
    tracing::debug!(user_id = %principal.user_id, "websocket closed");
}

async fn send(socket: &mut WebSocket, message: &ServerMessage) -> Result<(), axum::Error> {
    // Serialising these enums cannot fail (no maps with non-string keys, no custom impls).
    let json = serde_json::to_string(message).unwrap_or_default();
    socket.send(Message::Text(Utf8Bytes::from(json))).await
}
