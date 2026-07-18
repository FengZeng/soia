use crate::{json_value_to_string, mpv_command_checked, protocol::{CommandEnvelopeDto, CommandResultDto, CoreErrorDto, PlaybackSnapshotDto}, AppState};
use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, options, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use qrcode::QrCode;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tokio::net::TcpListener;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:17668";
const BIND_ENV_VAR: &str = "SOIA_REMOTE_CONTROL_ADDR";
const TOKEN_ENV_VAR: &str = "SOIA_REMOTE_CONTROL_TOKEN";
const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;

static REMOTE_CONTROL_ADDR: OnceLock<SocketAddr> = OnceLock::new();
static REMOTE_CONTROL_TOKEN: OnceLock<String> = OnceLock::new();
static REMOTE_CONTROL_RUNTIME: OnceLock<Arc<Mutex<RemoteControlRuntime>>> = OnceLock::new();
const PAIR_CODE_TTL: Duration = Duration::from_secs(60);

struct RemoteControlRuntime {
    enabled: bool,
    pair_code: Option<(String, Instant)>,
    sessions: std::collections::HashSet<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteControlInfo {
    pub url: String,
    pub qr_svg: String,
    pub enabled: bool,
}

#[derive(Clone)]
struct RemoteControlState {
    app_handle: tauri::AppHandle,
    token: Option<String>,
    web_root: PathBuf,
    runtime: Arc<Mutex<RemoteControlRuntime>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRequest { pair_code: String }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteControlStatus { enabled: bool, connected_devices: usize }

#[derive(Deserialize)]
#[serde(untagged)]
enum CommandRequest {
    Object { args: Vec<serde_json::Value> },
    Array(Vec<serde_json::Value>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WebSocketClientMessage {
    Command {
        envelope: CommandEnvelopeDto,
    },
    Navigation {
        id: Option<String>,
        action: String,
    },
    Ping {
        id: Option<String>,
    },
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

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum WebSocketServerMessage {
    Hello { protocol_version: u32 },
    State { state: PlaybackSnapshotDto },
    Pong { id: Option<String> },
    CommandResult { result: CommandResultDto },
    NavigationResult { id: Option<String>, ok: bool },
    Error { id: Option<String>, error: CoreErrorDto },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    error: String,
}

struct RemoteError {
    status: StatusCode,
    message: String,
}

impl RemoteError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }
}

impl IntoResponse for RemoteError {
    fn into_response(self) -> Response {
        with_cors((
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        ))
    }
}

pub(crate) fn start(app_handle: tauri::AppHandle) -> Result<(), String> {
    if REMOTE_CONTROL_ADDR.get().is_some() {
        return Ok(());
    }

    let bind_addr = std::env::var(BIND_ENV_VAR).unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let listener = StdTcpListener::bind(&bind_addr).map_err(|error| error.to_string())?;
    let addr = listener.local_addr().map_err(|error| error.to_string())?;
    let token = resolve_auth_token();
    let control_runtime = Arc::new(Mutex::new(RemoteControlRuntime {
        enabled: true,
        pair_code: None,
        sessions: Default::default(),
    }));
    let _ = REMOTE_CONTROL_RUNTIME.set(control_runtime.clone());
    let server_token = token.clone();
    validate_bind_addr(addr, token.as_deref())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;

    let app_handle_for_thread = app_handle.clone();
    std::thread::Builder::new()
        .name("soia-remote-control".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_io()
                .enable_time()
                .thread_name("soia-remote-control-worker")
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    warn!("remote control: failed to create async runtime: {error}");
                    return;
                }
            };
            runtime.block_on(async move {
                match TcpListener::from_std(listener) {
                    Ok(listener) => {
                        let router = build_router(app_handle_for_thread, server_token, control_runtime);
                        if let Err(error) = axum::serve(listener, router).await {
                            warn!("remote control: server failed: {error}");
                        }
                    }
                    Err(error) => warn!("remote control: failed to adopt listener: {error}"),
                }
            });
        })
        .map_err(|error| error.to_string())?;

    let _ = REMOTE_CONTROL_ADDR.set(addr);
    let _ = REMOTE_CONTROL_TOKEN.set(token.unwrap_or_default());
    info!("remote control: listening on http://{addr}");
    Ok(())
}

fn build_router(app_handle: tauri::AppHandle, token: Option<String>, runtime: Arc<Mutex<RemoteControlRuntime>>) -> Router {
    let web_root = remote_web_root(&app_handle);
    let state = RemoteControlState { app_handle, token, web_root, runtime };
    Router::new()
        .route("/health", get(health))
        .route("/remote", get(remote_page))
        .route("/remote/", get(remote_page))
        .route("/remote/*path", get(remote_asset))
        .route("/api/pair", post(pair))
        .route("/mpv/command", post(mpv_command).options(options_handler))
        .route("/ws", get(websocket))
        .route("/*path", options(options_handler))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

async fn health() -> Response {
    with_cors(Json(HealthResponse { ok: true }))
}

async fn remote_page(State(state): State<RemoteControlState>) -> Response {
    if !is_enabled(&state) { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
    serve_remote_file(&state.web_root, "remote.html")
}

async fn remote_asset(
    State(state): State<RemoteControlState>,
    Path(path): Path<String>,
) -> Response {
    if !is_enabled(&state) { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
    if path.split('/').any(|segment| segment == "..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve_remote_file(&state.web_root, &path)
}

async fn pair(
    State(state): State<RemoteControlState>,
    Json(payload): Json<PairRequest>,
) -> Result<Response, RemoteError> {
    let session = {
        let mut runtime = state.runtime.lock().map_err(|error| RemoteError::unauthorized(error.to_string()))?;
        if !runtime.enabled { return Err(RemoteError::unauthorized("remote control is disabled")); }
        let valid = runtime.pair_code.take().is_some_and(|(code, expires_at)| {
            expires_at > Instant::now() && code == payload.pair_code
        });
        if !valid { return Err(RemoteError::unauthorized("invalid or expired pairing code")); }
        let session = uuid::Uuid::now_v7().to_string();
        runtime.sessions.insert(session.clone());
        session
    };
    let cookie = format!("soia_remote_session={session}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400");
    Ok(([(header::SET_COOKIE, cookie)], Json(CommandResponse { ok: true })).into_response())
}

fn serve_remote_file(web_root: &std::path::Path, name: &str) -> Response {
    let path = web_root.join(name);
    let Ok(bytes) = std::fs::read(&path) else {
        warn!("remote control: web asset unavailable: {}", path.display());
        return (StatusCode::NOT_FOUND, "Remote web UI is not available in this build.").into_response();
    };
    let content_type = match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
}

fn remote_web_root(app_handle: &tauri::AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist");
    }
    app_handle
        .path()
        .resource_dir()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"))
        .join("dist")
}

async fn options_handler() -> Response {
    with_cors(StatusCode::NO_CONTENT)
}

async fn mpv_command(
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

async fn websocket(
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

async fn handle_websocket(socket: WebSocket, state: RemoteControlState, session: Option<String>) {
    let (mut sender, mut receiver) = socket.split();
    let hello = WebSocketServerMessage::Hello {
        protocol_version: 1,
    };
    if send_ws_json(&mut sender, &hello).await.is_err() {
        return;
    }

    let mut playback_state = {
        let app_state: tauri::State<'_, AppState> = state.app_handle.state();
        app_state.playback_state.subscribe()
    };
    let initial_state = playback_state.borrow().clone();
    if send_ws_json(&mut sender, &WebSocketServerMessage::State { state: initial_state }).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            changed = playback_state.changed() => {
                if changed.is_err() { return; }
                if !is_connection_active(&state, session.as_deref()) { return; }
                let next_state = playback_state.borrow().clone();
                if send_ws_json(&mut sender, &WebSocketServerMessage::State { state: next_state }).await.is_err() { return; }
            }
            message = receiver.next() => {
        let Some(message) = message else { return; };
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
                let response = handle_websocket_text(&state, &text);
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

fn handle_websocket_text(
    state: &RemoteControlState,
    text: &str,
) -> WebSocketServerMessage {
    match serde_json::from_str::<WebSocketClientMessage>(text) {
        Ok(WebSocketClientMessage::Ping { id }) => WebSocketServerMessage::Pong { id },
        Ok(WebSocketClientMessage::Command { envelope }) => {
            let id = Some(envelope.command_id.clone());
            let app_state: tauri::State<'_, AppState> = state.app_handle.state();
            match app_state.playback_service.execute(&app_state, envelope) {
                Ok(result) => WebSocketServerMessage::CommandResult { result },
                Err(error) => WebSocketServerMessage::Error {
                    id,
                    error,
                },
            }
        }
        Ok(WebSocketClientMessage::Navigation { id, action }) => match execute_remote_navigation(&state.app_handle, &action) {
            Ok(()) => WebSocketServerMessage::NavigationResult { id, ok: true },
            Err(error) => WebSocketServerMessage::Error {
                id,
                error: CoreErrorDto::ExecutionFailed { message: error },
            },
        },
        Err(error) => WebSocketServerMessage::Error {
            id: None,
            error: CoreErrorDto::InvalidCommand {
                message: format!("invalid websocket message: {error}"),
            },
        },
    }
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

fn execute_remote_navigation(app_handle: &tauri::AppHandle, action: &str) -> Result<(), String> {
    match action {
        "previous" => {
            app_handle
                .emit("soia-remote-playback-navigation", -1)
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        "next" => {
            app_handle
                .emit("soia-remote-playback-navigation", 1)
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        _ => return Err("unsupported remote navigation action".to_string()),
    }
}

fn resolve_auth_token() -> Option<String> {
    std::env::var(TOKEN_ENV_VAR)
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .or_else(|| Some(uuid::Uuid::now_v7().to_string()))
}

#[tauri::command]
pub(crate) fn get_remote_control_info() -> Result<RemoteControlInfo, String> {
    let addr = REMOTE_CONTROL_ADDR
        .get()
        .ok_or_else(|| "Remote control service is not running".to_string())?;
    let runtime = remote_runtime()?;
    let pair_code = {
        let mut runtime = runtime.lock().map_err(|error| error.to_string())?;
        if !runtime.enabled { return Err("Web Remote is disabled".to_string()); }
        let pair_code = uuid::Uuid::now_v7().to_string();
        runtime.pair_code = Some((pair_code.clone(), Instant::now() + PAIR_CODE_TTL));
        pair_code
    };
    let host = if addr.ip().is_unspecified() { local_network_ip()? } else { addr.ip().to_string() };
    let url = format!("http://{host}:{}/remote/#pair={pair_code}", addr.port());
    let qr_svg = QrCode::new(url.as_bytes())
        .map_err(|error| error.to_string())?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(qrcode::render::svg::Color("#15151b"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    Ok(RemoteControlInfo { url, qr_svg, enabled: true })
}

#[tauri::command]
pub(crate) fn get_remote_control_status() -> Result<RemoteControlStatus, String> {
    let runtime = remote_runtime()?;
    let runtime = runtime.lock().map_err(|error| error.to_string())?;
    Ok(RemoteControlStatus { enabled: runtime.enabled, connected_devices: runtime.sessions.len() })
}

#[tauri::command]
pub(crate) fn set_remote_control_enabled(enabled: bool) -> Result<RemoteControlStatus, String> {
    let runtime = remote_runtime()?;
    let mut runtime = runtime.lock().map_err(|error| error.to_string())?;
    runtime.enabled = enabled;
    runtime.pair_code = None;
    if !enabled { runtime.sessions.clear(); }
    Ok(RemoteControlStatus { enabled: runtime.enabled, connected_devices: runtime.sessions.len() })
}

#[tauri::command]
pub(crate) fn disconnect_remote_control_devices() -> Result<RemoteControlStatus, String> {
    let runtime = remote_runtime()?;
    let mut runtime = runtime.lock().map_err(|error| error.to_string())?;
    runtime.pair_code = None;
    runtime.sessions.clear();
    Ok(RemoteControlStatus { enabled: runtime.enabled, connected_devices: 0 })
}

fn remote_runtime() -> Result<&'static Arc<Mutex<RemoteControlRuntime>>, String> {
    REMOTE_CONTROL_RUNTIME.get().ok_or_else(|| "Remote control service is not running".to_string())
}

fn is_enabled(state: &RemoteControlState) -> bool {
    state.runtime.lock().map(|runtime| runtime.enabled).unwrap_or(false)
}

fn is_active_session(state: &RemoteControlState, session: &str) -> bool {
    state.runtime.lock().map(|runtime| runtime.sessions.contains(session)).unwrap_or(false)
}

fn is_connection_active(state: &RemoteControlState, session: Option<&str>) -> bool {
    is_enabled(state) && session.is_none_or(|session| is_active_session(state, session))
}

fn local_network_ip() -> Result<String, String> {
    let mut candidates: Vec<(u8, std::net::Ipv4Addr)> = get_if_addrs::get_if_addrs()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|interface| !is_virtual_interface(&interface.name))
        .filter_map(|interface| match interface.addr {
            get_if_addrs::IfAddr::V4(address) => Some(address.ip),
            get_if_addrs::IfAddr::V6(_) => None,
        })
        .filter(|ip| is_usable_private_ipv4(*ip))
        .map(|ip| (private_ip_priority(ip), ip))
        .collect();

    candidates.sort_by_key(|(priority, ip)| (*priority, *ip));
    candidates
        .first()
        .map(|(_, ip)| ip.to_string())
        .ok_or_else(|| "No private local network address found".to_string())
}

fn is_virtual_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["lo", "utun", "tun", "tap", "docker", "vbox", "vmnet", "bridge"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn is_usable_private_ipv4(ip: std::net::Ipv4Addr) -> bool {
    ip.is_private()
        && !ip.is_loopback()
        && !ip.is_unspecified()
        && !(ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))
}

fn private_ip_priority(ip: std::net::Ipv4Addr) -> u8 {
    match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        [172, 16..=31, ..] => 2,
        _ => 3,
    }
}

fn validate_bind_addr(addr: SocketAddr, token: Option<&str>) -> Result<(), String> {
    if addr.ip().is_loopback() || token.is_some() {
        return Ok(());
    }
    Err(format!(
        "refusing to bind remote control to {addr} without {TOKEN_ENV_VAR}"
    ))
}

fn authorize_headers(state: &RemoteControlState, headers: &HeaderMap) -> Result<(), RemoteError> {
    authorize(state, headers, None)
}

fn authorize(state: &RemoteControlState, headers: &HeaderMap, query_token: Option<&str>) -> Result<(), RemoteError> {
    if !is_enabled(state) { return Err(RemoteError::unauthorized("remote control is disabled")); }
    let session = headers.get(header::COOKIE).and_then(|value| value.to_str().ok()).and_then(remote_session_from_cookie);
    if session.is_some_and(|session| is_active_session(state, session)) {
        return Ok(());
    }
    let Some(expected_token) = state.token.as_deref() else {
        return Ok(());
    };

    let provided_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-soia-remote-token")
                .and_then(|value| value.to_str().ok())
        });

    if provided_token == Some(expected_token) || query_token == Some(expected_token) {
        Ok(())
    } else {
        Err(RemoteError::unauthorized("missing or invalid remote control token"))
    }
}

fn remote_session_from_cookie(cookie: &str) -> Option<&str> {
    cookie.split(';').map(str::trim).find_map(|part| part.strip_prefix("soia_remote_session="))
}

fn with_cors(response: impl IntoResponse) -> Response {
    let mut response = response.into_response();
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Authorization, Content-Type, X-Soia-Remote-Token"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    response
}
