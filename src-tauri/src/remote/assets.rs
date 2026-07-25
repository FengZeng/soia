use super::state::{is_enabled, RemoteControlState};
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use log::warn;
use std::path::PathBuf;
use tauri::Manager;

pub(super) async fn remote_page(State(state): State<RemoteControlState>) -> Response {
    if !is_enabled(&state) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    serve_remote_file(&state.web_root, "remote.html")
}

pub(super) async fn remote_asset(
    State(state): State<RemoteControlState>,
    Path(path): Path<String>,
) -> Response {
    if !is_enabled(&state) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if path.split('/').any(|segment| segment == "..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve_remote_file(&state.web_root, &path)
}

pub(super) fn remote_web_root(app_handle: &tauri::AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist");
    }
    app_handle
        .path()
        .resource_dir()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"))
        .join("dist")
}

fn serve_remote_file(web_root: &std::path::Path, name: &str) -> Response {
    let path = web_root.join(name);
    let Ok(bytes) = std::fs::read(&path) else {
        warn!("remote control: web asset unavailable: {}", path.display());
        return (
            StatusCode::NOT_FOUND,
            "Remote web UI is not available in this build.",
        )
            .into_response();
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
