use super::auth::{authorize, remote_session_from_cookie};
use super::state::{is_connection_active, RemoteControlState};
use crate::protocol::{
    CommandEnvelopeDto, CommandResultDto, CoreErrorDto, PlaybackSnapshotDto, PROTOCOL_VERSION,
};
use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::Manager;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WebSocketClientMessage {
    Command { envelope: CommandEnvelopeDto },
    Navigation { id: Option<String>, action: String },
    Ping { id: Option<String> },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(dead_code)]
enum WebSocketServerMessage {
    Hello { protocol_version: u32 },
    State { state: PlaybackSnapshotDto },
    Pong { id: Option<String> },
    CommandResult { result: CommandResultDto },
    NavigationResult { id: Option<String>, ok: bool },
    Error { id: Option<String>, error: CoreErrorDto },
}

pub(super) async fn websocket(
    State(state): State<RemoteControlState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let session = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(remote_session_from_cookie)
        .map(str::to_owned);
    match authorize(&state, &headers, query.get("token").map(String::as_str)) {
        Ok(()) => upgrade
            .on_upgrade(move |socket| handle_websocket(socket, state, session))
            .into_response(),
        Err(error) => error.into_response(),
    }
}

async fn handle_websocket(
    socket: WebSocket,
    state: RemoteControlState,
    session: Option<String>,
) {
    let (mut sender, mut receiver) = socket.split();
    let legacy_client_id = format!("remote-legacy-{}", uuid::Uuid::now_v7());
    let hello = WebSocketServerMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
    };
    if send_ws_json(&mut sender, &hello).await.is_err() {
        return;
    }

    let mut playback_state = {
        let app_state: tauri::State<'_, AppState> = state.app_handle.state();
        app_state.playback_state.subscribe()
    };
    let initial_state = playback_state.borrow().clone();
    if send_ws_json(
        &mut sender,
        &WebSocketServerMessage::State {
            state: initial_state,
        },
    )
    .await
    .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            changed = playback_state.changed() => {
                if changed.is_err() {
                    return;
                }
                if !is_connection_active(&state, session.as_deref()) {
                    return;
                }
                let next_state = playback_state.borrow().clone();
                if send_ws_json(&mut sender, &WebSocketServerMessage::State { state: next_state }).await.is_err() {
                    return;
                }
            }
            message = receiver.next() => {
                let Some(message) = message else {
                    return;
                };
                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        warn!("remote control: websocket receive failed: {error}");
                        return;
                    }
                };

                match message {
                    Message::Text(text) => {
                        if !is_connection_active(&state, session.as_deref()) {
                            return;
                        }
                        let response = handle_websocket_text(&state, &text, &legacy_client_id).await;
                        if send_ws_json(&mut sender, &response).await.is_err() {
                            return;
                        }
                    }
                    Message::Close(_) => return,
                    Message::Ping(payload) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            return;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_websocket_text(
    state: &RemoteControlState,
    text: &str,
    legacy_client_id: &str,
) -> WebSocketServerMessage {
    match serde_json::from_str::<WebSocketClientMessage>(text) {
        Ok(WebSocketClientMessage::Ping { id }) => WebSocketServerMessage::Pong { id },
        Ok(WebSocketClientMessage::Command { envelope }) => {
            let id = Some(envelope.command_id.clone());
            if crate::core::playback_service::PlaybackService::is_navigation_command(
                &envelope.command,
            ) {
                let app_state: tauri::State<'_, AppState> = state.app_handle.state();
                match crate::commands::navigation::execute_navigation_envelope(
                    &state.app_handle,
                    &app_state,
                    envelope,
                )
                .await
                {
                    Ok(result) => WebSocketServerMessage::CommandResult { result },
                    Err(error) => WebSocketServerMessage::Error { id, error },
                }
            } else {
                let app_state: tauri::State<'_, AppState> = state.app_handle.state();
                match app_state.playback_service.execute(&app_state, envelope).await {
                    Ok(result) => WebSocketServerMessage::CommandResult { result },
                    Err(error) => WebSocketServerMessage::Error { id, error },
                }
            }
        }
        Ok(WebSocketClientMessage::Navigation { id, action }) => {
            let envelope = navigation_action_to_envelope(&action, id.as_deref(), legacy_client_id);
            match envelope {
                Ok(envelope) => {
                    let command_id = Some(envelope.command_id.clone());
                    let app_state: tauri::State<'_, AppState> = state.app_handle.state();
                    match crate::commands::navigation::execute_navigation_envelope(
                        &state.app_handle,
                        &app_state,
                        envelope,
                    )
                    .await
                    {
                        Ok(result) => WebSocketServerMessage::CommandResult { result },
                        Err(error) => WebSocketServerMessage::Error {
                            id: command_id,
                            error,
                        },
                    }
                }
                Err(error) => WebSocketServerMessage::Error {
                    id,
                    error: CoreErrorDto::InvalidCommand { message: error },
                },
            }
        }
        Err(error) => WebSocketServerMessage::Error {
            id: None,
            error: CoreErrorDto::InvalidCommand {
                message: format!("invalid websocket message: {error}"),
            },
        },
    }
}

fn navigation_action_to_envelope(
    action: &str,
    id: Option<&str>,
    client_id: &str,
) -> Result<CommandEnvelopeDto, String> {
    let command = match action {
        "previous" => crate::protocol::PlaybackCommandDto::Previous,
        "next" => crate::protocol::PlaybackCommandDto::Next,
        _ => return Err(format!("unsupported navigation action: {action}")),
    };
    Ok(CommandEnvelopeDto {
        command_id: id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
        client_id: client_id.to_string(),
        playback_session_id: None,
        command,
    })
}

async fn send_ws_json(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &WebSocketServerMessage,
) -> Result<(), String> {
    let text = serde_json::to_string(message).map_err(|error| error.to_string())?;
    sender
        .send(Message::Text(text))
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{navigation_action_to_envelope, WebSocketServerMessage};
    use crate::protocol::{PlaybackCommandDto, PROTOCOL_VERSION};

    #[test]
    fn legacy_navigation_preserves_connection_and_request_identity() {
        let envelope = navigation_action_to_envelope(
            "next",
            Some("request-42"),
            "legacy-connection-7",
        )
        .expect("legacy navigation should map to a command envelope");

        assert_eq!(envelope.command_id, "request-42");
        assert_eq!(envelope.client_id, "legacy-connection-7");
        assert!(matches!(envelope.command, PlaybackCommandDto::Next));
    }

    #[test]
    fn hello_uses_the_current_protocol_version() {
        let message = serde_json::to_value(WebSocketServerMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
        })
        .expect("hello message should serialize");

        assert_eq!(message["type"], "hello");
        assert_eq!(message["protocol_version"], PROTOCOL_VERSION);
    }
}
