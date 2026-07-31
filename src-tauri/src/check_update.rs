use chrono::Utc;
use log::{info, warn};
use serde::Serialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

const UPDATE_AVAILABLE_EVENT: &str = "soia-update-available";
const UPDATE_NOTE_PROMPT_EVENT: &str = "soia-update-note-prompt";
static HAS_AVAILABLE_UPDATE: AtomicBool = AtomicBool::new(false);
static UPDATE_NOTE_PROMPT_LOCK: Mutex<()> = Mutex::new(());
static PENDING_UPDATE_NOTE_PROMPT: Mutex<Option<UpdateNotePrompt>> = Mutex::new(None);
const HOST_AUTH_CACHE_KEY: &str = "host_auth_v1";
const UPDATE_NOTE_PROMPTED_VERSION_KEY: &str = "update_note_prompted_version_v1";
const HOST_AUTH_EXPIRY_SKEW_SECS: i64 = 30;
const HOST_AUTH_CONNECT_TIMEOUT_SECS: u64 = 3;
const HOST_AUTH_TIMEOUT_SECS: u64 = 7;
#[cfg(target_os = "windows")]
const WINDOWS_PORTABLE_MARKER_FILE: &str = "marker.dll";
#[cfg(target_os = "windows")]
const WINDOWS_UNINSTALLER_FILE: &str = "uninstall.exe";

#[derive(Clone, Debug)]
pub(crate) struct SoiaAuthToken {
    pub payload: String,
    pub signature_hex: String,
    pub expire_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateNotePrompt {
    pub version: String,
    pub note: String,
}

#[tauri::command]
pub(crate) fn has_available_update() -> bool {
    HAS_AVAILABLE_UPDATE.load(Ordering::Relaxed)
}

#[tauri::command]
pub(crate) fn should_use_embedded_update_install() -> bool {
    #[cfg(not(target_os = "windows"))]
    {
        true
    }

    #[cfg(target_os = "windows")]
    {
        is_windows_setup_install()
    }
}

#[tauri::command]
pub(crate) fn consume_pending_update_note_prompt() -> Option<UpdateNotePrompt> {
    match PENDING_UPDATE_NOTE_PROMPT.lock() {
        Ok(mut prompt) => prompt.take(),
        Err(err) => {
            warn!("pending update note prompt lock is poisoned: {err}");
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn is_windows_setup_install() -> bool {
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
    else {
        return true;
    };
    let marker_exists = exe_dir.join(WINDOWS_PORTABLE_MARKER_FILE).is_file();
    let uninstall_exists = exe_dir.join(WINDOWS_UNINSTALLER_FILE).is_file();
    let is_portable = marker_exists && !uninstall_exists;
    !is_portable
}

fn set_update_available_state(available: bool) {
    HAS_AVAILABLE_UPDATE.store(available, Ordering::Relaxed);
}

fn soia_os_code() -> String {
    cfg_if::cfg_if! {
        if #[cfg(all(not(target_os = "macos"), target_arch = "aarch64"))] {
            const _OFFSET: i32 = 200;
        } else {
            const _OFFSET: i32 = 100;
        }
    }

    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            let os = if cfg!(target_arch = "aarch64") { 100 } else { 200 };
        } else if #[cfg(target_os = "windows")] {
            let os = 1 + _OFFSET;
        } else if #[cfg(target_os = "linux")] {
            let os = 2 + _OFFSET;
        } else if #[cfg(target_os = "android")] {
            let os = 3 + _OFFSET;
        } else {
            let os = 99 + _OFFSET;
        }
    }

    let mmdd = Utc::now().format("%m%d").to_string();
    let bytes = mmdd.as_bytes();
    if bytes.len() != 4 {
        return format!("00{os:03}00");
    }

    format!(
        "{}{}{:03}{}{}",
        bytes[1] as char, bytes[2] as char, os, bytes[0] as char, bytes[3] as char
    )
}

fn parse_payload_expire_at(payload: &str) -> Option<i64> {
    payload.split(';').find_map(|part| {
        let raw = part.strip_prefix("exp=")?;
        raw.trim().parse::<i64>().ok()
    })
}

fn parse_soia_auth_from_json(value: &serde_json::Value) -> Option<SoiaAuthToken> {
    let payload = value
        .get("auth_payload")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("payload").and_then(|v| v.as_str()))?
        .trim()
        .to_string();
    let signature_hex = value
        .get("auth_signature_hex")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("signature_hex").and_then(|v| v.as_str()))
        .or_else(|| value.get("signature").and_then(|v| v.as_str()))?
        .trim()
        .to_string();
    if payload.is_empty() || signature_hex.is_empty() {
        return None;
    }
    let expire_at = value
        .get("expire_at")
        .and_then(|v| v.as_i64())
        .or_else(|| parse_payload_expire_at(&payload))?;
    Some(SoiaAuthToken {
        payload,
        signature_hex,
        expire_at,
    })
}

fn read_cached_soia_auth(app_handle: &tauri::AppHandle) -> Option<SoiaAuthToken> {
    let state = crate::store::installation_store::get_installation_state(app_handle).ok()?;
    let token = state
        .uuid_update_data
        .get(HOST_AUTH_CACHE_KEY)
        .and_then(parse_soia_auth_from_json)?;
    let now = Utc::now().timestamp();
    (token.expire_at > now + HOST_AUTH_EXPIRY_SKEW_SECS).then_some(token)
}

fn persist_host_auth(app_handle: &tauri::AppHandle, token: &SoiaAuthToken) {
    let mut data = match read_uuid_update_data_object(app_handle, "host auth cache") {
        Some(data) => data,
        None => return,
    };
    data.insert(
        HOST_AUTH_CACHE_KEY.to_string(),
        json!({
            "auth_payload": token.payload,
            "auth_signature_hex": token.signature_hex,
            "expire_at": token.expire_at
        }),
    );
    persist_uuid_update_data_object(app_handle, data, "host auth cache");
}

fn read_uuid_update_data_object(
    app_handle: &tauri::AppHandle,
    context: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let state = match crate::store::installation_store::get_installation_state(app_handle) {
        Ok(state) => state,
        Err(err) => {
            warn!("failed to read installation state for {context}: {err}");
            return None;
        }
    };
    Some(
        state
            .uuid_update_data
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )
}

fn persist_uuid_update_data_object(
    app_handle: &tauri::AppHandle,
    data: serde_json::Map<String, serde_json::Value>,
    context: &str,
) {
    if let Err(err) = crate::store::installation_store::update_uuid_update_data(
        app_handle,
        serde_json::Value::Object(data),
    ) {
        warn!("failed to persist {context}: {err}");
    }
}

fn update_note_prompted_version(app_handle: &tauri::AppHandle) -> Option<String> {
    let state = crate::store::installation_store::get_installation_state(app_handle).ok()?;
    state
        .uuid_update_data
        .get(UPDATE_NOTE_PROMPTED_VERSION_KEY)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn mark_update_note_prompted(app_handle: &tauri::AppHandle, version: &str) {
    let mut data = match read_uuid_update_data_object(app_handle, "update note prompt state") {
        Some(data) => data,
        None => return,
    };
    data.insert(
        UPDATE_NOTE_PROMPTED_VERSION_KEY.to_string(),
        serde_json::Value::String(version.to_string()),
    );
    persist_uuid_update_data_object(app_handle, data, "update note prompt state");
}

fn parse_soia_api_base_urls(raw: &str) -> Vec<String> {
    raw.split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.contains("://") {
                value.to_string()
            } else {
                format!("https://{value}")
            }
        })
        .collect()
}

fn request_soia_auth_from_server(app_handle: &tauri::AppHandle) -> Option<SoiaAuthToken> {
    let state: crate::store::installation_store::InstallationState =
        crate::store::installation_store::get_installation_state(app_handle).ok()?;
    let base_urls = parse_soia_api_base_urls(option_env!("SOIA_API").unwrap_or(""));
    if base_urls.is_empty() {
        return None;
    }

    let version_query = format!("id={}&v={}", state.install_id, env!("CARGO_PKG_VERSION"));
    let os = soia_os_code();
    let client = crate::network::proxy::configure_blocking_client_builder(
        app_handle,
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(HOST_AUTH_CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(HOST_AUTH_TIMEOUT_SECS)),
    )
    .ok()?
    .build()
        .ok()?;

    for base_url in base_urls {
        let separator = if base_url.contains('?') { '&' } else { '?' };
        let url = format!("{base_url}{separator}{version_query}&os={os}");
        let response = match client.get(&url).send() {
            Ok(response) => response,
            Err(_) => {
                continue;
            }
        };
        if !response.status().is_success() {
            continue;
        }

        let body = match response.text() {
            Ok(body) => body,
            Err(_) => {
                continue;
            }
        };
        let payload: serde_json::Value = match serde_json::from_str(&body) {
            Ok(payload) => payload,
            Err(_) => {
                continue;
            }
        };
        let Some(token) = parse_soia_auth_from_json(&payload).or_else(|| {
            payload
                .get("data")
                .and_then(parse_soia_auth_from_json)
                .or_else(|| payload.get("result").and_then(parse_soia_auth_from_json))
        }) else {
            continue;
        };

        persist_host_auth(app_handle, &token);
        if let Err(err) = crate::store::installation_store::mark_daily_signal_reported(app_handle) {
            warn!("mark_daily_signal_reported failed after successful host auth fetch: {err}");
        }
        return Some(token);
    }

    None
}

pub(crate) fn run_daily_signal_ping(app_handle: &tauri::AppHandle) -> Option<SoiaAuthToken> {
    let cached_token = read_cached_soia_auth(app_handle);

    let daily = match crate::store::installation_store::mark_daily_signal(app_handle) {
        Ok(result) => result,
        Err(err) => {
            warn!("mark_daily_signal failed, skip daily signal ping: {err}");
            return cached_token;
        }
    };

    if !daily.should_run {
        return cached_token;
    }

    let new_token = request_soia_auth_from_server(app_handle);

    new_token.or(cached_token)
}

fn run_daily_update_check(app_handle: &tauri::AppHandle) {
    match crate::store::installation_store::mark_daily_update_check(app_handle) {
        Ok(result) if result.should_run => {
            #[cfg(desktop)]
            {
                set_update_available_state(false);
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    run_tauri_updater_check(app_handle).await;
                });
            }
        }
        Ok(_) => {}
        Err(err) => {
            warn!("mark_daily_update_check failed: {err}");
        }
    }
}

#[cfg(desktop)]
fn update_note_from_raw_json(update: &tauri_plugin_updater::Update) -> Option<String> {
    update
        .raw_json
        .get("notes")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(desktop)]
fn available_update_note(update: &tauri_plugin_updater::Update) -> Option<String> {
    let note = update
        .body
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| update_note_from_raw_json(update))?;

    has_detailed_update_note(&note).then_some(note)
}

#[cfg(desktop)]
fn has_detailed_update_note(note: &str) -> bool {
    note.lines().filter(|line| !line.trim().is_empty()).count() > 1
}

#[cfg(desktop)]
fn maybe_show_update_note_prompt(
    app_handle: &tauri::AppHandle,
    update: &tauri_plugin_updater::Update,
) {
    let version = update.version.trim();
    if version.is_empty() {
        return;
    }

    let Some(note) = available_update_note(update) else {
        return;
    };

    let _prompt_lock = match UPDATE_NOTE_PROMPT_LOCK.lock() {
        Ok(lock) => lock,
        Err(err) => {
            warn!("update note prompt lock is poisoned: {err}");
            return;
        }
    };
    if update_note_prompted_version(app_handle).as_deref() == Some(version) {
        return;
    }

    mark_update_note_prompted(app_handle, version);

    use tauri::Emitter;

    let prompt = UpdateNotePrompt {
        version: version.to_string(),
        note,
    };
    match PENDING_UPDATE_NOTE_PROMPT.lock() {
        Ok(mut pending_prompt) => {
            *pending_prompt = Some(prompt.clone());
        }
        Err(err) => {
            warn!("pending update note prompt lock is poisoned: {err}");
        }
    }
    let _ = app_handle.emit(UPDATE_NOTE_PROMPT_EVENT, prompt);
}

pub(crate) fn check_update(app_handle: tauri::AppHandle) -> Option<SoiaAuthToken> {
    let result = run_daily_signal_ping(&app_handle);
    run_daily_update_check(&app_handle);
    result
}

#[cfg(target_os = "macos")]
pub(crate) fn check_update_now(app_handle: tauri::AppHandle) {
    set_update_available_state(false);
    tauri::async_runtime::spawn(async move {
        run_tauri_updater_check(app_handle).await;
    });
}

#[cfg(desktop)]
async fn run_tauri_updater_check(app_handle: tauri::AppHandle) {
    use tauri::Emitter;
    use tauri_plugin_updater::UpdaterExt;

    let updater = match app_handle.updater_builder().build() {
        Ok(updater) => updater,
        Err(tauri_plugin_updater::Error::EmptyEndpoints) => {
            info!("tauri updater skipped: no endpoints configured");
            return;
        }
        Err(err) => {
            warn!("failed to build tauri updater: {err}");
            return;
        }
    };

    match updater.check().await {
        Ok(Some(update)) => {
            set_update_available_state(true);
            let _ = app_handle.emit(UPDATE_AVAILABLE_EVENT, true);
            maybe_show_update_note_prompt(&app_handle, &update);
            info!(
                "tauri updater found an available update: {}",
                update.version
            );
        }
        Ok(None) => {
            set_update_available_state(false);
            if let Err(err) =
                crate::store::installation_store::mark_daily_update_check_success(&app_handle)
            {
                warn!("failed to persist last_update_checked_date_utc: {err}");
            }
            let _ = app_handle.emit(UPDATE_AVAILABLE_EVENT, false);
            info!("tauri updater: no update available");
        }
        Err(err) => {
            warn!("tauri updater check failed: {err}");
        }
    }
}
