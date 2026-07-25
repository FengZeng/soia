use super::auth::{authorize_headers, with_cors, RemoteError};
use super::state::RemoteControlState;
use crate::{json_value_to_string, mpv_command_checked, AppState};
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tauri::Manager;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PairRequest {
    pair_code: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum CommandRequest {
    Object { args: Vec<serde_json::Value> },
    Array(Vec<serde_json::Value>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    ok: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandResponse {
    ok: bool,
}

pub(super) async fn health() -> Response {
    with_cors(Json(HealthResponse { ok: true }))
}

pub(super) async fn pair(
    State(state): State<RemoteControlState>,
    Json(payload): Json<PairRequest>,
) -> Result<Response, RemoteError> {
    let session = {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|error| RemoteError::unauthorized(error.to_string()))?;
        if !runtime.enabled {
            return Err(RemoteError::unauthorized("remote control is disabled"));
        }
        let valid = runtime.pair_code.take().is_some_and(|(code, expires_at)| {
            expires_at > Instant::now() && code == payload.pair_code
        });
        if !valid {
            return Err(RemoteError::unauthorized("invalid or expired pairing code"));
        }
        let session = uuid::Uuid::now_v7().to_string();
        runtime.sessions.insert(session.clone());
        session
    };
    let cookie = format!(
        "soia_remote_session={session}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400"
    );
    Ok(([(header::SET_COOKIE, cookie)], Json(CommandResponse { ok: true })).into_response())
}

pub(super) async fn options_handler() -> Response {
    with_cors(StatusCode::NO_CONTENT)
}

pub(super) async fn mpv_command(
    State(state): State<RemoteControlState>,
    headers: HeaderMap,
    payload: Result<Json<CommandRequest>, JsonRejection>,
) -> Result<Response, RemoteError> {
    authorize_headers(&state, &headers)?;
    let Json(command_request) =
        payload.map_err(|error| RemoteError::bad_request(format!("invalid JSON: {error}")))?;
    execute_mpv_command(&state.app_handle, command_request).map_err(RemoteError::bad_request)?;
    Ok(with_cors(Json(CommandResponse { ok: true })))
}

fn execute_mpv_command(
    app_handle: &tauri::AppHandle,
    command_request: CommandRequest,
) -> Result<(), String> {
    let args = match command_request {
        CommandRequest::Object { args } => args,
        CommandRequest::Array(args) => args,
    };
    if args.is_empty() {
        return Err("mpv command args cannot be empty".to_string());
    }

    let args_owned: Vec<String> = args
        .into_iter()
        .map(json_value_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let args_str: Vec<&str> = args_owned.iter().map(String::as_str).collect();
    let state: tauri::State<'_, AppState> = app_handle.state();
    let mpv_guard = state.mpv_player.lock().map_err(|error| error.to_string())?;
    mpv_command_checked(&mpv_guard, &args_str)
}
