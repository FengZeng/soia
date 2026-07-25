use super::assets::remote_web_root;
use super::routes::{health, mpv_command, options_handler, pair};
use super::state::{
    resolve_auth_token, RemoteControlRuntime, RemoteControlState, REMOTE_CONTROL_ADDR,
    REMOTE_CONTROL_RUNTIME, REMOTE_CONTROL_TOKEN,
};
use super::websocket::websocket;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, options, post};
use axum::Router;
use log::{info, warn};
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:17668";
const BIND_ENV_VAR: &str = "SOIA_REMOTE_CONTROL_ADDR";
const TOKEN_ENV_VAR: &str = "SOIA_REMOTE_CONTROL_TOKEN";
const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;

pub(crate) fn start(app_handle: tauri::AppHandle) -> Result<(), String> {
    if REMOTE_CONTROL_ADDR.get().is_some() {
        return Ok(());
    }

    let bind_addr = std::env::var(BIND_ENV_VAR).unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let listener = StdTcpListener::bind(&bind_addr).map_err(|error| error.to_string())?;
    let addr = listener.local_addr().map_err(|error| error.to_string())?;
    let token = resolve_auth_token(TOKEN_ENV_VAR);
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
                        let router = build_router(
                            app_handle_for_thread,
                            server_token,
                            control_runtime,
                        );
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

fn build_router(
    app_handle: tauri::AppHandle,
    token: Option<String>,
    runtime: Arc<Mutex<RemoteControlRuntime>>,
) -> Router {
    let web_root = remote_web_root(&app_handle);
    let state = RemoteControlState {
        app_handle,
        token,
        web_root,
        runtime,
    };
    Router::new()
        .route("/health", get(health))
        .route("/remote", get(super::assets::remote_page))
        .route("/remote/", get(super::assets::remote_page))
        .route("/remote/*path", get(super::assets::remote_asset))
        .route("/api/pair", post(pair))
        .route("/mpv/command", post(mpv_command).options(options_handler))
        .route("/ws", get(websocket))
        .route("/*path", options(options_handler))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

fn validate_bind_addr(addr: SocketAddr, token: Option<&str>) -> Result<(), String> {
    if addr.ip().is_loopback() || token.is_some() {
        return Ok(());
    }
    Err(format!(
        "refusing to bind remote control to {addr} without {TOKEN_ENV_VAR}"
    ))
}
