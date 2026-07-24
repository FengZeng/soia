// Tauri imports
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod app_bootstrap;
pub mod protocol;
mod core;
mod check_update;
mod commands;
mod media_extensions;
mod mpv;
mod network;
mod online_subtitles;
mod platform;
mod playback_source;
mod remote_control;
mod shader_pipeline;
mod subtitles;
use mpv::MpvHandle;
use tauri::{Emitter, Listener, Manager};
mod store;

const MAIN_WINDOW_LABEL: &str = "main";
const FRONTEND_READY_EVENT: &str = "soia-frontend-ready";
const STARTUP_WINDOW_SHOW_FALLBACK_MS: u64 = 3000;

pub struct AppState {
    pub mpv_player: Arc<Mutex<MpvHandle>>,
    pub pending_play_history_entry: Mutex<Option<store::play_history::PlayHistoryEntry>>,
    pub current_playback_key: Mutex<Option<String>>,
    pub(crate) pending_playback_loads: Mutex<VecDeque<(u64, String)>>,
    now_playing: Mutex<NowPlayingState>,
    pub(crate) playback_command_lock: tokio::sync::Mutex<()>,
    pub(crate) playback_state: core::state::PlaybackStatePublisher,
    pub(crate) playback_service: core::playback_service::PlaybackService,
    pub(crate) playback_load_coordinator: core::playback_loading::PlaybackLoadCoordinator,
    pub(crate) navigation_service: core::navigation::NavigationService,
    pub(crate) shader_pipeline: shader_pipeline::ShaderPipeline,
}

#[derive(Default)]
pub struct OpenFileState {
    pub pending_paths: Mutex<Vec<String>>,
}

#[derive(Clone, Default)]
struct NowPlayingState {
    title: Option<String>,
    duration: Option<f64>,
    position: f64,
    is_playing: bool,
    artwork_path: Option<String>,
}

type AppResult<T> = Result<T, String>;

fn init_logging() {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().filter_or("RUST_LOG", "info"));
    builder
        .filter_module("reqwest", log::LevelFilter::Warn)
        .filter_module("hyper", log::LevelFilter::Warn)
        .filter_module("hyper_util", log::LevelFilter::Warn)
        .filter_module("tauri_plugin_updater", log::LevelFilter::Warn);
    builder.format(|buf, record| {
        use std::io::Write;

        let time = chrono::Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).expect("valid UTC+08:00"))
            .format("%H:%M:%S%.3f");

        let level = match record.level() {
            log::Level::Error => "e",
            log::Level::Warn => "w",
            log::Level::Info => "i",
            log::Level::Debug => "d",
            log::Level::Trace => "t",
        };

        writeln!(buf, "{} [{}] {}", time, level, record.args())
    });
    let _ = builder.try_init();
}

fn json_value_to_string(value: serde_json::Value) -> AppResult<String> {
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(if b { "yes".into() } else { "no".into() }),
        _ => Err("Unsupported value type".into()),
    }
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> AppResult<std::sync::MutexGuard<'_, T>> {
    mutex.lock().map_err(|e| e.to_string())
}

fn with_mpv<R>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&MpvHandle) -> AppResult<R>,
) -> AppResult<R> {
    let mpv_guard = lock_mutex(state.mpv_player.as_ref())?;
    f(&mpv_guard)
}

pub(crate) fn stage_pending_play_history_entry(
    state: &tauri::State<'_, AppState>,
    entry: store::play_history::PlayHistoryEntry,
) -> AppResult<()> {
    let mut pending = lock_mutex(&state.pending_play_history_entry)?;
    *pending = Some(entry);
    Ok(())
}

pub(crate) fn clear_pending_play_history_entry(
    state: &tauri::State<'_, AppState>,
    path: Option<String>,
) -> AppResult<()> {
    let mut pending = lock_mutex(&state.pending_play_history_entry)?;
    match path {
        Some(path) if pending.as_ref().is_some_and(|entry| entry.path == path) => {
            *pending = None;
        }
        None => {
            *pending = None;
        }
        _ => {}
    }
    Ok(())
}

fn refresh_pending_play_history_entry(
    entry: &mut store::play_history::PlayHistoryEntry,
    current_playback_key: Option<&str>,
    snapshot: protocol::PlaybackSnapshotDto,
) {
    if current_playback_key != Some(entry.path.as_str()) {
        return;
    }
    if snapshot.position.is_finite() && snapshot.position > 0.0 {
        entry.last_position = snapshot.position;
    }
    if snapshot.duration.is_finite() && snapshot.duration > 0.0 {
        entry.duration = snapshot.duration;
    }
    if entry.title.trim().is_empty() {
        if let Some(title) = snapshot.title.filter(|title| !title.trim().is_empty()) {
            entry.title = title;
        }
    }
}

pub(crate) fn flush_pending_play_history_entry(app_handle: &tauri::AppHandle) -> AppResult<()> {
    let state: tauri::State<'_, AppState> = app_handle.state::<AppState>();
    let entry = {
        let mut pending = lock_mutex(&state.pending_play_history_entry)?;
        pending.take()
    };
    let Some(mut entry) = entry else {
        return Ok(());
    };
    let current_playback_key = lock_mutex(&state.current_playback_key)?.clone();
    refresh_pending_play_history_entry(
        &mut entry,
        current_playback_key.as_deref(),
        state.playback_state.current(),
    );

    if let Err(error) =
        store::play_history::save_play_history_progress_entry(app_handle, entry.clone())
    {
        if let Ok(mut pending) = state.pending_play_history_entry.lock() {
            if pending.is_none() {
                *pending = Some(entry);
            }
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod pending_play_history_tests {
    use super::refresh_pending_play_history_entry;
    use crate::protocol::PlaybackSnapshotDto;
    use crate::store::play_history::PlayHistoryEntry;

    fn history_entry(path: &str) -> PlayHistoryEntry {
        PlayHistoryEntry {
            id: String::new(),
            path: path.to_string(),
            title: String::new(),
            last_position: 10.0,
            duration: 100.0,
            last_played_at: 0,
            is_pinned: false,
            is_live_playback: false,
            external_audio_tracks: Vec::new(),
            external_sub_tracks: Vec::new(),
        }
    }

    #[test]
    fn refreshes_matching_pending_history_from_core_snapshot() {
        let mut entry = history_entry("media-key");
        let snapshot = PlaybackSnapshotDto {
            title: Some("Core title".to_string()),
            position: 42.0,
            duration: 120.0,
            ..PlaybackSnapshotDto::default()
        };

        refresh_pending_play_history_entry(&mut entry, Some("media-key"), snapshot);

        assert_eq!(entry.last_position, 42.0);
        assert_eq!(entry.duration, 120.0);
        assert_eq!(entry.title, "Core title");
    }

    #[test]
    fn leaves_mismatched_pending_history_unchanged() {
        let mut entry = history_entry("older-media");
        let snapshot = PlaybackSnapshotDto {
            position: 42.0,
            duration: 120.0,
            ..PlaybackSnapshotDto::default()
        };

        refresh_pending_play_history_entry(&mut entry, Some("current-media"), snapshot);

        assert_eq!(entry.last_position, 10.0);
        assert_eq!(entry.duration, 100.0);
    }
}

fn with_now_playing_mut<R>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&mut NowPlayingState) -> R,
) -> AppResult<R> {
    let mut now_playing = lock_mutex(&state.now_playing)?;
    Ok(f(&mut now_playing))
}

fn mpv_command_checked(mpv: &MpvHandle, args: &[&str]) -> AppResult<()> {
    let rewritten_args = rewrite_mpv_command_urls(args);
    let command_args: Vec<&str> = rewritten_args
        .as_ref()
        .map(|args| args.iter().map(String::as_str).collect())
        .unwrap_or_else(|| args.to_vec());
    let result_code = mpv.command(&command_args);
    if result_code == 0 {
        Ok(())
    } else {
        let log_args = redact_mpv_command_args(&command_args);
        Err(format!(
            "MPV command {:?} failed with error code: {}",
            log_args, result_code
        ))
    }
}

fn mpv_command_direct_checked(mpv: &MpvHandle, args: &[&str]) -> AppResult<()> {
    let result_code = mpv.command(args);
    if result_code == 0 {
        Ok(())
    } else {
        let log_args = redact_mpv_command_args(args);
        Err(format!(
            "MPV command {:?} failed with error code: {}",
            log_args, result_code
        ))
    }
}

fn redact_mpv_command_args(args: &[&str]) -> Vec<String> {
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            if index == 1 && args.first().copied() == Some("loadfile") {
                redact_url(arg)
            } else {
                (*arg).to_string()
            }
        })
        .collect()
}

fn redact_url(raw: &str) -> String {
    let Ok(mut url) = url::Url::parse(raw) else {
        return raw.to_string();
    };
    if !url.username().is_empty() {
        let _ = url.set_username("<user>");
        let _ = url.set_password(Some("<redacted>"));
    }
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn rewrite_mpv_command_urls(args: &[&str]) -> Option<Vec<String>> {
    if args.len() < 2 || args.first().copied() != Some("loadfile") {
        return None;
    }

    // Remote protocol credentials, headers, cookies, and connection state belong in stream_proxy
    // backends. mpv should receive only localhost token URLs so secrets do not leak into mpv
    // command logs, options, or protocol-specific URL handling.
    let rewritten_url = crate::mpv::rewrite_http_stream_url(args[1])
        .or_else(|| crate::mpv::rewrite_https_stream_url(args[1]))
        .or_else(|| crate::mpv::rewrite_smb_stream_url(args[1]))
        .or_else(|| crate::mpv::rewrite_https_callback_url(args[1]))?;
    let mut rewritten: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    rewritten[1] = rewritten_url;
    Some(rewritten)
}

fn mpv_set_option_string_checked(mpv: &MpvHandle, name: &str, value: &str) -> AppResult<()> {
    let result_code = mpv.set_option_string(name, value);
    if result_code >= 0 {
        Ok(())
    } else {
        Err(format!(
            "MPV set_option_string({}, {}) failed with error code: {}",
            name, value, result_code
        ))
    }
}

fn build_artwork_file_name(url: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    let hash = hasher.finish();
    let url_path = PathBuf::from(url);
    let base_name = url_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("media");
    let safe_name: String = base_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{safe_name}_{hash:016x}.jpg")
}

fn normalize_open_media_path(path: &Path) -> Option<String> {
    if path.as_os_str().is_empty() || path.is_dir() {
        return None;
    }
    Some(path.to_string_lossy().into_owned())
}

fn collect_open_media_paths_from_args() -> Vec<String> {
    std::env::args_os()
        .skip(1)
        .filter_map(|arg| {
            if let Some(raw) = arg.to_str() {
                // macOS launches GUI apps with a process serial number arg.
                if raw.starts_with("-psn_") {
                    return None;
                }
            }
            let path = PathBuf::from(arg);
            normalize_open_media_path(&path)
        })
        .collect()
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn collect_open_media_paths_from_urls(urls: Vec<url::Url>) -> Vec<String> {
    urls.into_iter()
        .filter_map(|url| {
            if let Ok(path) = url.to_file_path() {
                return normalize_open_media_path(&path);
            }
            if url.scheme() != "file" {
                return None;
            }
            let decoded = percent_encoding::percent_decode_str(url.path())
                .decode_utf8()
                .ok()?;
            let candidate = PathBuf::from(decoded.as_ref());
            normalize_open_media_path(&candidate)
        })
        .collect()
}

fn queue_open_media_paths(app: &tauri::AppHandle, paths: Vec<String>, emit_event: bool) {
    if paths.is_empty() {
        return;
    }

    let mut deduped = Vec::new();
    for path in paths {
        if deduped.contains(&path) {
            continue;
        }
        deduped.push(path);
    }

    let mut queued_any = false;
    if let Some(open_state) = app.try_state::<OpenFileState>() {
        if let Ok(mut pending) = open_state.pending_paths.lock() {
            for path in deduped {
                if pending.contains(&path) {
                    continue;
                }
                pending.push(path);
                queued_any = true;
            }
        }
    }

    // Notify frontend to drain queue. If frontend isn't ready yet, paths stay queued.
    if emit_event && queued_any {
        let _ = app.emit("app-open-files", ());
    }
}

fn handle_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if let tauri::RunEvent::Opened { urls } = event {
        let paths = collect_open_media_paths_from_urls(urls);
        queue_open_media_paths(app_handle, paths, true);
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = (app_handle, event);
}

#[cfg(desktop)]
fn show_main_window(app_handle: &tauri::AppHandle) {
    let app_handle_for_thread = app_handle.clone();
    let app_handle_for_show = app_handle.clone();
    let _ = app_handle_for_thread.run_on_main_thread(move || {
        if let Some(window) = app_handle_for_show.get_webview_window(MAIN_WINDOW_LABEL) {
            let _ = window.show();
        }
    });
}

#[cfg(desktop)]
fn install_frontend_ready_window_show(app: &mut tauri::App) {
    let app_handle = app.handle().clone();
    app.listen(FRONTEND_READY_EVENT, move |_| {
        show_main_window(&app_handle);
    });

    let app_handle = app.handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(
            STARTUP_WINDOW_SHOW_FALLBACK_MS,
        ));
        show_main_window(&app_handle);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

    let mut builder = tauri::Builder::default()
        .manage(OpenFileState::default())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            let startup_paths = collect_open_media_paths_from_args();
            queue_open_media_paths(&app.handle(), startup_paths, false);
            app_bootstrap::setup(app)?;
            #[cfg(desktop)]
            install_frontend_ready_window_show(app);
            Ok(())
        })
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_process::init());
    }

    let app = builder
        .invoke_handler(tauri::generate_handler![
            commands::playback::mpv_run_command,
            commands::playback::mpv_set_option_string,
            commands::playback::load_playback_source,
            commands::platform::pick_media_paths_native,
            commands::platform::pick_paths_native,
            commands::playback::consume_pending_open_files,
            commands::playback::cycle_pause,
            commands::playback::seek_video,
            commands::playback::execute_playback_command,
            commands::window::set_window_controls_visible,
            commands::window::apply_window_appearance,
            commands::window::set_window_vibrancy_visible,
            commands::window::sync_mpv_render_target,
            commands::platform::is_native_pip_enabled,
            commands::platform::set_native_pip_enabled,
            commands::playback::get_runtime_versions,
            commands::playback::get_media_file_size,
            commands::playback::list_local_media_siblings,
            playback_source::resolve::resolve_playback_source,
            playback_source::adjacency::resolve_adjacent_playback_source,
            subtitles::find_fuzzy_external_subtitle_matches,
            online_subtitles::search_online_subtitles,
            online_subtitles::download_online_subtitle,
            online_subtitles::clear_online_subtitle_cache,
            commands::playback::parse_playlist_file,
            commands::playback::parse_playlist_source,
            commands::playback::resolve_youtube_playlist,
            commands::network::list_network_connections,
            commands::network::save_network_connection,
            commands::network::delete_network_connection,
            commands::network::discover_network_connections,
            commands::network::browse_network_connection,
            commands::now_playing::set_now_playing_metadata,
            commands::now_playing::clear_now_playing,
            commands::now_playing::capture_now_playing_artwork,
            remote_control::get_remote_control_info,
            remote_control::get_remote_control_status,
            remote_control::set_remote_control_enabled,
            remote_control::disconnect_remote_control_devices,
            commands::navigation::sync_navigation_state,
            commands::navigation::execute_navigation_command,
            commands::persistence::load_play_history,
            commands::persistence::save_play_history,
            commands::persistence::save_play_history_entry,
            commands::persistence::stage_play_history_entry,
            commands::persistence::clear_staged_play_history_entry,
            commands::persistence::get_installation_state,
            commands::persistence::update_uuid_update_data,
            commands::persistence::factory_reset,
            commands::persistence::mark_daily_signal,
            commands::persistence::mark_daily_update_check,
            commands::persistence::load_ui_state,
            commands::persistence::save_ui_state,
            commands::persistence::open_log_directory,
            commands::persistence::apply_logging_settings,
            commands::persistence::apply_ytdl_settings,
            commands::persistence::apply_proxy_settings,
            commands::persistence::apply_stream_proxy_settings,
            commands::persistence::apply_rendering_settings,
            commands::persistence::set_brightness_adjustment,
            commands::persistence::resolve_shader_candidates,
            commands::persistence::resolve_existing_shader_files,
            commands::persistence::get_media_association_status,
            commands::persistence::set_media_association_to_soia,
            check_update::has_available_update,
            check_update::should_use_embedded_update_install,
            check_update::consume_pending_update_note_prompt
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(handle_run_event);
}
