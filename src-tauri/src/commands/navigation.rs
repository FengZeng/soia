use crate::protocol::{CommandEnvelopeDto, CommandResultDto, CoreErrorDto, PlaybackCommandDto};
use crate::AppState;
use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;
use tauri::Manager;

const NAVIGATION_RESULT_CACHE_CAPACITY: usize = 64;

struct CachedNavResult {
    client_id: String,
    command_id: String,
    result: Result<CommandResultDto, CoreErrorDto>,
}

static NAV_RESULT_CACHE: std::sync::OnceLock<StdMutex<VecDeque<CachedNavResult>>> =
    std::sync::OnceLock::new();

fn nav_cache() -> &'static StdMutex<VecDeque<CachedNavResult>> {
    NAV_RESULT_CACHE.get_or_init(|| StdMutex::new(VecDeque::with_capacity(NAVIGATION_RESULT_CACHE_CAPACITY)))
}

fn lookup_cached(client_id: &str, command_id: &str) -> Option<Result<CommandResultDto, CoreErrorDto>> {
    nav_cache()
        .lock()
        .ok()?
        .iter()
        .find(|entry| entry.client_id == client_id && entry.command_id == command_id)
        .map(|entry| entry.result.clone())
}

fn store_cached(client_id: &str, command_id: &str, result: Result<CommandResultDto, CoreErrorDto>) {
    let Ok(mut cache) = nav_cache().lock() else { return };
    if cache.len() == NAVIGATION_RESULT_CACHE_CAPACITY {
        cache.pop_front();
    }
    cache.push_back(CachedNavResult {
        client_id: client_id.to_string(),
        command_id: command_id.to_string(),
        result,
    });
}

/// Tauri command for Desktop to execute navigation through Core.
/// Uses the same serialized path as Browser/WebSocket navigation.
#[tauri::command]
pub(crate) async fn execute_navigation_command(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    envelope: CommandEnvelopeDto,
) -> Result<CommandResultDto, CoreErrorDto> {
    execute_navigation_envelope(&app, &state, envelope).await
}

/// Unified entry point for navigation commands from any client (Desktop, Browser, Mobile).
/// Serializes execution, deduplicates by commandId, and resolves + loads the target.
pub(crate) async fn execute_navigation_envelope(
    app: &tauri::AppHandle,
    state: &AppState,
    envelope: CommandEnvelopeDto,
) -> Result<CommandResultDto, CoreErrorDto> {
    if envelope.command_id.trim().is_empty() || envelope.client_id.trim().is_empty() {
        return Err(CoreErrorDto::InvalidCommand {
            message: "commandId and clientId are required".into(),
        });
    }

    // Check commandId dedup cache before acquiring the lock
    if let Some(cached) = lookup_cached(&envelope.client_id, &envelope.command_id) {
        return cached;
    }

    // Serialize all Core playback commands through one admission lock.
    let _lock = state.playback_command_lock.lock().await;

    // Re-check cache after acquiring lock (another request may have completed)
    if let Some(cached) = lookup_cached(&envelope.client_id, &envelope.command_id) {
        return cached;
    }

    let direction = match &envelope.command {
        PlaybackCommandDto::Previous => -1,
        PlaybackCommandDto::Next => 1,
        PlaybackCommandDto::PlaySource { key, title } => {
            let result = execute_play_source(app, state, key.clone(), title.clone()).await;
            let mapped = result.map(|rev| CommandResultDto {
                command_id: envelope.command_id.clone(),
                applied_snapshot_revision: rev,
            });
            store_cached(&envelope.client_id, &envelope.command_id, mapped.clone());
            return mapped;
        }
        _ => {
            return Err(CoreErrorDto::InvalidCommand {
                message: "only Previous, Next, and PlaySource are navigation commands".into(),
            });
        }
    };

    let result = execute_direction(app, state, direction).await.map(|rev| CommandResultDto {
        command_id: envelope.command_id.clone(),
        applied_snapshot_revision: rev,
    });
    store_cached(&envelope.client_id, &envelope.command_id, result.clone());
    result
}

/// Execute previous/next navigation: stop playback, resolve the target, then load it.
async fn execute_direction(
    app: &tauri::AppHandle,
    state: &AppState,
    direction: i32,
) -> Result<u64, CoreErrorDto> {
    let current_key = state
        .current_playback_key
        .lock()
        .map_err(|e| CoreErrorDto::ExecutionFailed { message: e.to_string() })?
        .clone()
        .ok_or_else(|| CoreErrorDto::NavigationFailed {
            message: "no current playback source".into(),
        })?;

    if current_key.trim().is_empty() {
        return Err(CoreErrorDto::NavigationFailed {
            message: "no current playback source".into(),
        });
    }

    crate::commands::playback::publish_source_load_state(
        app,
        state,
        true,
        Some(current_key.clone()),
        None,
    );

    // Stop immediately so the current source does not keep playing while a
    // remote target (for example YouTube) is being resolved and prepared.
    {
        let mpv = match state.mpv_player.lock() {
            Ok(mpv) => mpv,
            Err(error) => {
                let message = error.to_string();
                crate::commands::playback::publish_source_load_state(
                    app,
                    state,
                    false,
                    Some(current_key.clone()),
                    Some(message.clone()),
                );
                return Err(CoreErrorDto::ExecutionFailed { message });
            }
        };
        if let Err(error) = crate::mpv_command_checked(&mpv, &["stop"]) {
            let message = format!("failed to stop current playback: {error}");
            crate::commands::playback::publish_source_load_state(
                app,
                state,
                false,
                Some(current_key.clone()),
                Some(message.clone()),
            );
            return Err(CoreErrorDto::NavigationFailed { message });
        }
    }

    // Step 1: resolve playlist navigation from the SQLite authority. NavigationService supplies
    // only the current UI/runtime context; it no longer supplies playlist entries for this path.
    let playlist_context = state.navigation_service.playlist_navigation_context();
    let playlist_result = state
        .playlist_service
        .resolve_navigation(app, &current_key, direction, &playlist_context, false)
        .map_err(|message| CoreErrorDto::NavigationFailed { message })?;
    if let Some(result) = &playlist_result {
        state
            .navigation_service
            .set_playback_playlist_id(Some(result.playlist_id.clone()));
    }

    // Step 2: Fallback to directory adjacency if no playlist match
    let (target_key, title) = if let Some(nav_result) = playlist_result {
        (nav_result.path, nav_result.title)
    } else {
        let adjacent = match crate::playback_source::adjacency::resolve_adjacent_key(
            app,
            &current_key,
            direction,
        )
        .await
        {
            Ok(adjacent) => adjacent,
            Err(message) => {
                crate::commands::playback::publish_source_load_state(
                    app,
                    state,
                    false,
                    Some(current_key.clone()),
                    Some(message.clone()),
                );
                return Err(CoreErrorDto::NavigationFailed { message });
            }
        };
        match adjacent {
            Some(source) => {
                let title = state
                    .playlist_service
                    .title_for_path(
                        app,
                        &source.playback_key,
                        &state.navigation_service.playlist_navigation_context(),
                    )
                    .unwrap_or(None)
                    .or(source.title);
                (source.playback_key, title)
            }
            None => {
                let message = "no adjacent media found".to_string();
                crate::commands::playback::publish_source_load_state(
                    app,
                    state,
                    false,
                    Some(current_key),
                    Some(message.clone()),
                );
                return Err(CoreErrorDto::NavigationFailed {
                    message,
                });
            }
        }
    };

    // Step 3: Load the resolved source
    load_and_get_revision(app, state, target_key, title).await
}

/// Execute a PlaySource command: load the specified key directly.
async fn execute_play_source(
    app: &tauri::AppHandle,
    state: &AppState,
    key: String,
    title: Option<String>,
) -> Result<u64, CoreErrorDto> {
    if key.trim().is_empty() {
        return Err(CoreErrorDto::InvalidCommand {
            message: "playSource key cannot be empty".into(),
        });
    }
    load_and_get_revision(app, state, key, title).await
}

/// Load a source and return the resulting snapshot revision.
async fn load_and_get_revision(
    app: &tauri::AppHandle,
    state: &AppState,
    key: String,
    title: Option<String>,
) -> Result<u64, CoreErrorDto> {
    let payload = crate::commands::playback::LoadPlaybackSourcePayload::new(key, title);
    crate::commands::playback::load_source(app, state, payload)
        .await
        .map(|_| state.playback_state.current().revision)
        .map_err(|e| CoreErrorDto::NavigationFailed { message: e })
}

const AUTO_PLAY_NEXT_SETTING_LABEL: &str = "AUTO_PLAY_NEXT_IN_PLAYLIST";

/// Called by the mpv event loop when a track ends naturally (EOF).
/// The generation and key are bound to mpv's FILE_LOADED event. After acquiring
/// the shared playback command lock, both must still be current; otherwise a
/// newer playback request has superseded this EOF action.
pub(crate) async fn handle_end_of_file(
    app: &tauri::AppHandle,
    ended_generation: u64,
    ended_key: &str,
) {
    let state: tauri::State<'_, AppState> = app.state();

    // Check auto-play setting
    if !is_auto_play_enabled(app) {
        return;
    }

    // Serialize EOF navigation with every other Core playback command.
    let _lock = state.playback_command_lock.lock().await;

    if state.playback_load_coordinator.current() != ended_generation {
        return;
    }

    // Verify the media hasn't changed since EOF was fired. If the user (or
    // another client) loaded a new track between EOF and now, discard this.
    let current_key = match state.current_playback_key.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return,
    };
    match current_key.as_deref() {
        Some(key) if key == ended_key => {}
        _ => {
            // Media changed — discard stale EOF action
            return;
        }
    }

    // Resolve from the SQLite playlist authority (including loop-one semantics).
    let playlist_context = state.navigation_service.playlist_navigation_context();
    let playlist_result = match state
        .playlist_service
        .resolve_navigation(app, ended_key, 1, &playlist_context, true)
    {
        Ok(result) => result,
        Err(error) => {
            log::warn!("failed to resolve playlist EOF navigation: {error}");
            None
        }
    };
    if let Some(result) = &playlist_result {
        state
            .navigation_service
            .set_playback_playlist_id(Some(result.playlist_id.clone()));
    }

    let (target_key, title) = if let Some(nav_result) = playlist_result {
        (nav_result.path, nav_result.title)
    } else {
        // Fallback: directory adjacency (forward only)
        let adjacent = crate::playback_source::adjacency::resolve_adjacent_key(
            app,
            ended_key,
            1,
        )
        .await
        .unwrap_or(None);
        match adjacent {
            Some(source) => {
                let title = state
                    .playlist_service
                    .title_for_path(
                        app,
                        &source.playback_key,
                        &state.navigation_service.playlist_navigation_context(),
                    )
                    .unwrap_or(None)
                    .or(source.title);
                (source.playback_key, title)
            }
            None => return, // No next track — playback ends
        }
    };

    // Load the next track
    let payload = crate::commands::playback::LoadPlaybackSourcePayload::new(target_key, title);
    if let Err(error) = crate::commands::playback::load_source(app, &state, payload).await {
        log::warn!("auto-play next after EOF failed: {error}");
    }
}

fn is_auto_play_enabled(app: &tauri::AppHandle) -> bool {
    match crate::store::ui_state_store::load_setting_value(app, AUTO_PLAY_NEXT_SETTING_LABEL) {
        Ok(Some(value)) => value != "Off",
        Ok(None) => true, // Default is enabled
        Err(_) => true,
    }
}
