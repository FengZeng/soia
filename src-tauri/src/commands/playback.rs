use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;

use crate::protocol::{
    ContinuePlaylistSourceOperationDto, PlaylistSourceClientActionDto,
    PlaylistSourceContinuationResultDto, PreparePlaylistSourceOperationDto,
    PreparePlaylistSourceOperationResultDto,
};

use crate::{
    json_value_to_string, mpv_command_checked, mpv_set_option_string_checked, with_mpv, AppState,
    OpenFileState,
};

fn with_mpv_state<R>(
    state: &AppState,
    f: impl FnOnce(&crate::mpv::MpvHandle) -> Result<R, String>,
) -> Result<R, String> {
    let mpv_guard = state.mpv_player.lock().map_err(|e| e.to_string())?;
    f(&mpv_guard)
}

#[tauri::command]
pub(crate) async fn execute_playback_command(
    state: tauri::State<'_, AppState>,
    envelope: crate::protocol::CommandEnvelopeDto,
) -> Result<crate::protocol::CommandResultDto, crate::protocol::CoreErrorDto> {
    state.playback_service.execute(&state, envelope).await
}

#[tauri::command]
pub(crate) fn get_playback_snapshot(
    state: tauri::State<'_, AppState>,
) -> crate::protocol::PlaybackSnapshotDto {
    state.playback_state.current()
}

#[tauri::command]
pub(crate) fn mpv_run_command(
    state: tauri::State<'_, AppState>,
    args: Vec<serde_json::Value>,
) -> Result<(), String> {
    let args_owned: Vec<String> = args
        .iter()
        .cloned()
        .map(json_value_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let args_str: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();
    with_mpv(&state, |mpv_guard| {
        mpv_command_checked(mpv_guard, &args_str)
    })
}

#[tauri::command]
pub(crate) fn mpv_set_option_string(
    state: tauri::State<'_, AppState>,
    name: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let value_str = json_value_to_string(value)?;
    with_mpv(&state, |mpv_guard| {
        mpv_set_option_string_checked(mpv_guard, &name, &value_str)
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoadPlaybackSourcePayload {
    key_or_url: String,
    #[serde(default)]
    preferred_title: Option<String>,
}

impl LoadPlaybackSourcePayload {
    pub(crate) fn new(key_or_url: String, preferred_title: Option<String>) -> Self {
        Self {
            key_or_url,
            preferred_title,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoadPlaybackSourceResult {
    playback_key: Option<String>,
    title: Option<String>,
    is_live_playback: bool,
    superseded: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackLoadPreparedPayload {
    playback_key: String,
    resume_position: f64,
}

fn superseded_load_result() -> LoadPlaybackSourceResult {
    LoadPlaybackSourceResult {
        playback_key: None,
        title: None,
        is_live_playback: false,
        superseded: true,
    }
}

pub(crate) fn publish_source_load_state(
    app: &tauri::AppHandle,
    state: &AppState,
    loading: bool,
    loading_key: Option<String>,
    error: Option<String>,
) {
    let snapshot = state.playback_state.update(|snapshot| {
        snapshot.source_loading = loading;
        snapshot.source_loading_key = loading_key;
        snapshot.source_load_error = error;
    });
    if let Err(error) = app.emit("playback-snapshot", snapshot) {
        log::warn!("failed to emit source load snapshot: {error}");
    }
}

fn source_load_error_or_superseded(
    app: &tauri::AppHandle,
    state: &AppState,
    generation: u64,
    source_key: &str,
    error: String,
) -> Result<LoadPlaybackSourceResult, String> {
    state
        .playback_load_coordinator
        .execute_if_current(generation, || {
            publish_source_load_state(
                app,
                state,
                false,
                Some(source_key.to_string()),
                Some(error.clone()),
            );
            Err(error)
        })
        .unwrap_or_else(|| Ok(superseded_load_result()))
}

const SKIP_INTRO_SECONDS_SETTING_LABEL: &str = "SKIP_INTRO_SECONDS";

fn parse_skip_intro_seconds(value: Option<String>) -> f64 {
    value
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(0.0)
}

fn load_skip_intro_seconds(app: &tauri::AppHandle) -> f64 {
    match crate::store::ui_state_store::load_setting_value(
        app,
        SKIP_INTRO_SECONDS_SETTING_LABEL,
    ) {
        Ok(value) => parse_skip_intro_seconds(value),
        Err(error) => {
            log::warn!("failed to load skip-intro setting: {error}");
            0.0
        }
    }
}

#[tauri::command]
pub(crate) async fn load_playback_source(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    payload: LoadPlaybackSourcePayload,
) -> Result<LoadPlaybackSourceResult, String> {
    let _admission_lock = state.playback_command_lock.lock().await;
    load_source(&app, &state, payload).await
}

/// Recognizes and prepares a playlist source without returning its entries to the client.
/// Core completes direct playback itself and only returns an action when a client confirmation is
/// required before persisting a prepared playlist.
#[tauri::command]
pub(crate) async fn prepare_playlist_source_operation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: PreparePlaylistSourceOperationDto,
) -> Result<PreparePlaylistSourceOperationResultDto, String> {
    let client_id = request.client_id.trim();
    if client_id.is_empty() {
        return Err("playlist source clientId is required".to_string());
    }
    let sources = normalize_playlist_sources(request.sources);
    if sources.is_empty() {
        return Err("playlist source selection is empty".to_string());
    }

    let preferred_title = request.preferred_title.and_then(normalize_optional_title);
    let prepared = prepare_playlist_source_operation_data(&app, &state, sources, preferred_title).await?;
    let PreparedPlaylistSourceOperation::ClientActionRequired {
        prepared_playlist,
        playback_key,
        playback_title,
        source_label,
        is_live_playback,
        playlist_entry_count,
    } = prepared else {
        let PreparedPlaylistSourceOperation::DirectPlayback {
            playback_key,
            playback_title,
            is_live_playback,
            playlist_entry_count,
        } = prepared else { unreachable!() };
        let _admission_lock = state.playback_command_lock.lock().await;
        let playback = load_source(
            &app,
            &state,
            LoadPlaybackSourcePayload::new(playback_key, playback_title),
        )
        .await?;
        return Ok(PreparePlaylistSourceOperationResultDto::Completed {
            result: PlaylistSourceContinuationResultDto {
                playlist_id: None,
                playback_key: playback.playback_key,
                title: playback.title,
                is_live_playback: is_live_playback.unwrap_or(playback.is_live_playback),
                superseded: playback.superseded,
            },
            playlist_entry_count,
        });
    };
    let item_count = u32::try_from(unique_prepared_entry_count(&prepared_playlist))
        .map_err(|_| "playlist source has too many entries".to_string())?;
    if item_count == 0 {
        return Err("playlist source contains no playable entries".to_string());
    }
    let suggested_name = prepared_playlist.name.clone();
    let operation_id = state.playlist_source_operations.begin(
        client_id.to_string(),
        prepared_playlist,
        playback_key,
        playback_title,
    )?;
    Ok(PreparePlaylistSourceOperationResultDto::ClientActionRequired {
        action: PlaylistSourceClientActionDto {
            operation_id,
            suggested_name,
            item_count,
            source_label: Some(source_label),
        },
        is_live_playback,
        playlist_entry_count,
    })
}

enum PreparedPlaylistSourceOperation {
    DirectPlayback {
        playback_key: String,
        playback_title: Option<String>,
        is_live_playback: Option<bool>,
        playlist_entry_count: Option<u32>,
    },
    ClientActionRequired {
        prepared_playlist: crate::core::playlist_service::PreparedPlaylist,
        playback_key: String,
        playback_title: Option<String>,
        source_label: String,
        is_live_playback: Option<bool>,
        playlist_entry_count: Option<u32>,
    },
}

/// Applies a client-local confirmation to a prepared operation. Import and playback are performed
/// by Core so a failed persistence transaction cannot leave an empty playlist behind.
#[tauri::command]
pub(crate) async fn continue_playlist_source_operation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: ContinuePlaylistSourceOperationDto,
) -> Result<PlaylistSourceContinuationResultDto, String> {
    let client_id = request.client_id.trim();
    let operation_id = request.operation_id.trim();
    if client_id.is_empty() || operation_id.is_empty() {
        return Err("playlist source clientId and operationId are required".to_string());
    }
    let claim = state
        .playlist_source_operations
        .claim(client_id, operation_id)?;
    let crate::core::playlist_source_operations::PlaylistSourceOperationClaim::Execute {
        mut prepared_playlist,
        playback_key,
        playback_title,
    } = claim else {
        let crate::core::playlist_source_operations::PlaylistSourceOperationClaim::Completed(outcome) = claim else { unreachable!() };
        return outcome;
    };
    let _admission_lock = state.playback_command_lock.lock().await;
    let playlist_id = if request.create_playlist {
        if let Some(name) = request.playlist_name {
            let name = name.trim();
            if !name.is_empty() {
                prepared_playlist.name = name.to_string();
            }
        }
        let playlist = state
            .playlist_service
            .import_prepared_playlist_as_playback(&app, prepared_playlist);
        let playlist = match playlist {
            Ok(playlist) => playlist,
            Err(error) => {
                state.playlist_source_operations.release(client_id, operation_id)?;
                return Err(error);
            }
        };
        state
            .navigation_service
            .set_playback_playlist_id(Some(playlist.id.clone()));
        let snapshot = match state.playlist_service.publish_snapshot(&app) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                state.playlist_source_operations.complete(
                    client_id.to_string(), operation_id.to_string(), Err(error.clone()),
                )?;
                return Err(error);
            }
        };
        if let Err(error) = app.emit("playlist-snapshot", snapshot) {
            let error = error.to_string();
            state.playlist_source_operations.complete(
                client_id.to_string(), operation_id.to_string(), Err(error.clone()),
            )?;
            return Err(error);
        }
        Some(playlist.id)
    } else {
        None
    };
    let playback = load_source(
        &app,
        &state,
        LoadPlaybackSourcePayload::new(playback_key, playback_title),
    ).await;
    let playback = match playback {
        Ok(playback) => playback,
        Err(error) => {
            if playlist_id.is_some() {
                state.playlist_source_operations.complete(
                    client_id.to_string(), operation_id.to_string(), Err(error.clone()),
                )?;
            } else {
                state.playlist_source_operations.release(client_id, operation_id)?;
            }
            return Err(error);
        }
    };
    let result = PlaylistSourceContinuationResultDto {
        playlist_id,
        playback_key: playback.playback_key,
        title: playback.title,
        is_live_playback: playback.is_live_playback,
        superseded: playback.superseded,
    };
    state.playlist_source_operations.complete(
        client_id.to_string(),
        operation_id.to_string(),
        Ok(result.clone()),
    )?;
    Ok(result)
}

fn normalize_playlist_sources(sources: Vec<String>) -> Vec<String> {
    let mut unique = std::collections::HashSet::new();
    sources
        .into_iter()
        .map(|source| source.trim().to_string())
        .filter(|source| !source.is_empty() && unique.insert(source.clone()))
        .collect()
}

fn unique_prepared_entry_count(prepared_playlist: &crate::core::playlist_service::PreparedPlaylist) -> usize {
    prepared_playlist
        .entries
        .iter()
        .map(|entry| entry.path.trim())
        .filter(|path| !path.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .len()
}

async fn prepare_playlist_source_operation_data(
    app: &tauri::AppHandle,
    state: &AppState,
    sources: Vec<String>,
    preferred_title: Option<String>,
) -> Result<PreparedPlaylistSourceOperation, String> {
    if sources.len() > 1 {
        let fallback_name = next_default_playlist_name(state, app)?;
        let name = derive_playlist_name_from_paths(&sources, &fallback_name);
        let entries = sources
            .iter()
            .enumerate()
            .map(|(index, path)| crate::core::playlist_service::PreparedPlaylistEntry {
                path: path.clone(),
                title: None,
                artwork_ref: None,
                added_at: crate::store::media_db::now_millis() + index as i64,
            })
            .collect();
        return Ok(PreparedPlaylistSourceOperation::ClientActionRequired {
            prepared_playlist: crate::core::playlist_service::PreparedPlaylist { name, entries },
            playback_key: sources[0].clone(),
            playback_title: preferred_title,
            source_label: common_playlist_source_label(&sources),
            is_live_playback: None,
            playlist_entry_count: None,
        });
    }

    let source = &sources[0];
    if is_youtube_playlist_source(source) {
        let resolved = match crate::mpv::resolve_ytdlp_playlist(app, source).await {
            Ok(resolved) => resolved,
            Err(error) => {
                log::warn!("YouTube playlist preparation fell back to original source: {error}");
                return Ok(direct_playlist_source_playback(
                    source.clone(),
                    preferred_title,
                    None,
                    None,
                ));
            }
        };
        let Some(first_entry) = resolved.entries.first() else {
            log::warn!("YouTube playlist preparation fell back to original source: no playable entries");
            return Ok(direct_playlist_source_playback(
                source.clone(),
                preferred_title,
                None,
                None,
            ));
        };
        let playback_key = first_entry.url.clone();
        let playback_title = first_entry.title.clone();
        let entries = resolved
            .entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| crate::core::playlist_service::PreparedPlaylistEntry {
                path: entry.url,
                title: entry.title,
                artwork_ref: None,
                added_at: crate::store::media_db::now_millis() + index as i64,
            })
            .collect();
        return Ok(PreparedPlaylistSourceOperation::ClientActionRequired {
            prepared_playlist: crate::core::playlist_service::PreparedPlaylist {
                name: resolved.title.unwrap_or_else(|| "YouTube Playlist".to_string()),
                entries,
            },
            playback_key,
            playback_title: playback_title.or(preferred_title),
            source_label: source.clone(),
            is_live_playback: None,
            playlist_entry_count: None,
        });
    }

    if !is_playlist_source(source) {
        return Ok(direct_playlist_source_playback(
            source.clone(),
            preferred_title,
            None,
            None,
        ));
    }
    let parsed = match parse_playlist_source_inner(app, source) {
        Ok(parsed) => parsed,
        Err(error) => {
            log::warn!("playlist source preparation fell back to original source: {error}");
            return Ok(direct_playlist_source_playback(
                source.clone(),
                preferred_title,
                None,
                None,
            ));
        }
    };
    if parsed.metadata.has_hls_tags {
        return Ok(direct_playlist_source_playback(
            source.clone(),
            preferred_title,
            Some(is_live_hls_playlist(&parsed.metadata)),
            None,
        ));
    }
    let Some(first_entry) = parsed.entries.first() else {
        return Ok(direct_playlist_source_playback(
            source.clone(),
            preferred_title,
            None,
            None,
        ));
    };
    if parsed.entries.len() == 1 {
        return Ok(direct_playlist_source_playback(
            first_entry.path.clone(),
            first_entry.title.clone().or(preferred_title),
            Some(true),
            Some(1),
        ));
    }
    let fallback_name = next_default_playlist_name(state, app)?;
    let name = playlist_name_from_source(source).unwrap_or(fallback_name);
    let playback_key = first_entry.path.clone();
    let playback_title = first_entry.title.clone().or(preferred_title);
    let playlist_entry_count = u32::try_from(parsed.entries.len())
        .map_err(|_| "playlist source has too many entries".to_string())?;
    let entries = parsed
        .entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| crate::core::playlist_service::PreparedPlaylistEntry {
            path: entry.path,
            title: entry.title,
            artwork_ref: entry.icon,
            added_at: crate::store::media_db::now_millis() + index as i64,
        })
        .collect();
    Ok(PreparedPlaylistSourceOperation::ClientActionRequired {
        prepared_playlist: crate::core::playlist_service::PreparedPlaylist { name, entries },
        playback_key,
        playback_title,
        source_label: source.clone(),
        is_live_playback: Some(true),
        playlist_entry_count: Some(playlist_entry_count),
    })
}

fn direct_playlist_source_playback(
    playback_key: String,
    playback_title: Option<String>,
    is_live_playback: Option<bool>,
    playlist_entry_count: Option<u32>,
) -> PreparedPlaylistSourceOperation {
    PreparedPlaylistSourceOperation::DirectPlayback {
        playback_key,
        playback_title,
        is_live_playback,
        playlist_entry_count,
    }
}

fn is_playlist_source(source: &str) -> bool {
    let trimmed = source.trim();
    let path = url::Url::parse(trimmed)
        .ok()
        .map(|url| url.path().to_string())
        .unwrap_or_else(|| trimmed.to_string());
    let path = path.to_ascii_lowercase();
    path.ends_with(".m3u") || path.ends_with(".m3u8")
}

fn is_live_hls_playlist(metadata: &ParsedPlaylistMetadata) -> bool {
    !metadata.has_end_list
        && !matches!(metadata.playlist_type.as_deref(), Some(value) if value.eq_ignore_ascii_case("VOD"))
}

fn is_youtube_playlist_source(source: &str) -> bool {
    let Ok(url) = url::Url::parse(source.trim()) else { return false; };
    matches!(url.host_str(), Some("youtube.com" | "www.youtube.com" | "music.youtube.com"))
        && (url.path() == "/playlist" || url.path().starts_with("/show/"))
}

fn playlist_name_from_source(source: &str) -> Option<String> {
    let path = url::Url::parse(source)
        .ok()
        .map(|url| url.path().to_string())
        .unwrap_or_else(|| source.to_string());
    Path::new(&path)
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn next_default_playlist_name(state: &AppState, app: &tauri::AppHandle) -> Result<String, String> {
    let count = state
        .playlist_service
        .list_summaries(app)?
        .into_iter()
        .filter(|playlist| !playlist.is_protected)
        .count();
    Ok(format!("Playlist {}", count + 1))
}

fn normalize_optional_title(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn derive_playlist_name_from_paths(paths: &[String], fallback: &str) -> String {
    let item_names = paths
        .iter()
        .filter_map(|path| playlist_name_from_source(path))
        .collect::<Vec<_>>();
    if item_names.is_empty() {
        return fallback.to_string();
    }
    if item_names.iter().all(|name| name.chars().all(|character| character.is_ascii_digit())) {
        if let Some(parent) = paths.first().and_then(|path| Path::new(path).parent()).and_then(|path| path.file_name()).and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()) {
            return parent.to_string();
        }
    }
    if item_names.len() == 1 {
        return item_names[0].clone();
    }
    let mut prefix = item_names[0].clone();
    for name in item_names.iter().skip(1) {
        while !prefix.is_empty() && !name.starts_with(&prefix) {
            prefix.pop();
        }
    }
    let prefix = prefix.trim_end_matches(|character: char| character.is_whitespace() || matches!(character, '.' | '_' | '-')).trim();
    if prefix.chars().count() >= 2 { prefix.to_string() } else { item_names[0].clone() }
}

fn common_playlist_source_label(sources: &[String]) -> String {
    let first = sources.first().cloned().unwrap_or_default();
    let parent = Path::new(&first).parent().and_then(|path| path.to_str());
    let label = match parent {
        Some(parent) if sources.iter().all(|source| Path::new(source).parent().and_then(|path| path.to_str()) == Some(parent)) => parent.to_string(),
        _ => first,
    };
    collapse_home_playlist_source_label(&label)
}

fn collapse_home_playlist_source_label(value: &str) -> String {
    let Some(rest) = value.strip_prefix("/Users/") else { return value.to_string(); };
    let Some(separator) = rest.find('/') else { return value.to_string(); };
    format!("~{}", &rest[separator..])
}

/// Core loading logic usable from both the Tauri command and internal navigation.
pub(crate) async fn load_source(
    app: &tauri::AppHandle,
    state: &AppState,
    payload: LoadPlaybackSourcePayload,
) -> Result<LoadPlaybackSourceResult, String> {
    let request_key = payload.key_or_url.trim().to_string();
    let preferred_title = payload.preferred_title.and_then(|t| {
        let trimmed = t.trim().to_string();
        if trimmed.is_empty() { None } else { Some(trimmed) }
    });
    if request_key.is_empty() {
        return Err("playback source cannot be empty".to_string());
    }
    let generation = state.playback_load_coordinator.begin();
    let _ = state
        .playback_load_coordinator
        .execute_if_current(generation, || {
            publish_source_load_state(&app, &state, true, Some(request_key.clone()), None);
        });
    if let Err(error) = crate::flush_pending_play_history_entry(&app) {
        log::warn!("failed to flush playback history before loading source: {error}");
    }
    let source = match crate::playback_source::resolve::resolve(&app, &request_key).await {
        Ok(source) => source,
        Err(error) => {
            return source_load_error_or_superseded(
                &app,
                &state,
                generation,
                &request_key,
                error,
            );
        }
    };
    let source_key = source.playback_key().to_string();
    if state
        .playback_load_coordinator
        .execute_if_current(generation, || {
            publish_source_load_state(&app, &state, true, Some(source_key.clone()), None);
        })
        .is_none()
    {
        return Ok(superseded_load_result());
    }
    let skip_intro_seconds = load_skip_intro_seconds(&app);
    let resume_position = match crate::store::play_history::resolve_resume_position(
        &app,
        &source_key,
        skip_intro_seconds,
    ) {
        Ok(position) => position,
        Err(error) => {
            log::warn!("failed to resolve playback resume position: {error}");
            if skip_intro_seconds.is_finite() {
                skip_intro_seconds.max(0.0)
            } else {
                0.0
            }
        }
    };
    let playback_speed = state.playback_state.current().speed;
    let load_options = match crate::core::playback_loading::PlaybackLoadOptions::from_optional(
        Some(resume_position),
        None,
        Some(playback_speed),
    ) {
        Ok(options) => options,
        Err(error) => {
            return source_load_error_or_superseded(
                &app,
                &state,
                generation,
                &source_key,
                error,
            );
        }
    };
    let prepared = match crate::playback_source::load::prepare(&app, source).await {
        Ok(prepared) => prepared,
        Err(error) => {
            return source_load_error_or_superseded(
                &app,
                &state,
                generation,
                &source_key,
                error,
            );
        }
    };
    let mpv_load_options = prepared.mpv_load_options.clone();
    let effective_title = preferred_title.clone().or(prepared.title.clone());
    let load_result = state
        .playback_load_coordinator
        .execute_if_current(generation, || {
            if let Err(error) = app.emit(
                "playback-load-prepared",
                PlaybackLoadPreparedPayload {
                    playback_key: source_key.clone(),
                    resume_position,
                },
            ) {
                log::warn!("failed to emit prepared playback load: {error}");
            }
            {
                let mut pending_loads = state
                    .pending_playback_loads
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                pending_loads.push_back((generation, source_key.clone()));
            }
            match with_mpv_state(state, |mpv_guard| {
                mpv_set_option_string_checked(
                    mpv_guard,
                    "force-media-title",
                    effective_title.as_deref().unwrap_or(""),
                )?;
                crate::core::playback_loading::load(
                    mpv_guard,
                    &prepared.playback_url,
                    &mpv_load_options,
                    load_options,
                    prepared.command_mode,
                )
            }) {
                Ok(()) => {
                    let playback_session_id = uuid::Uuid::now_v7().to_string();
                    {
                        let mut current_playback_key = state
                            .current_playback_key
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        *current_playback_key = Some(source_key.clone());
                    }
                    // Use preferred_title (from playlist) if available, otherwise
                    // fall back to the title resolved by the source preparer.
                    let playback_playlist_id = state
                        .navigation_service
                        .playlist_navigation_context()
                        .playback_playlist_id;
                    let snapshot = state.playback_state.update(|snapshot| {
                        snapshot.playback_session_id = Some(playback_session_id);
                        snapshot.playback_key = Some(source_key.clone());
                        snapshot.playback_playlist_id = playback_playlist_id;
                        snapshot.source_loading = false;
                        snapshot.source_loading_key = None;
                        snapshot.source_load_error = None;
                        if let Some(ref title) = effective_title {
                            snapshot.title = Some(title.clone());
                        }
                    });
                    if let Err(error) = app.emit("playback-snapshot", snapshot) {
                        log::warn!("failed to emit loaded source snapshot: {error}");
                    }
                    Ok(LoadPlaybackSourceResult {
                        playback_key: Some(source_key.clone()),
                        title: effective_title,
                        is_live_playback: prepared.is_live_playback,
                        superseded: false,
                    })
                }
                Err(error) => {
                    {
                        let mut pending_loads = state
                            .pending_playback_loads
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        pending_loads.retain(|(pending_generation, _)| {
                            *pending_generation != generation
                        });
                    }
                    publish_source_load_state(
                        &app,
                        &state,
                        false,
                        Some(source_key),
                        Some(error.clone()),
                    );
                    Err(error)
                }
            }
        });
    load_result.unwrap_or_else(|| Ok(superseded_load_result()))
}

#[cfg(test)]
mod tests {
    use super::parse_skip_intro_seconds;

    #[test]
    fn parses_positive_skip_intro_seconds() {
        assert_eq!(parse_skip_intro_seconds(Some("15.5".to_string())), 15.5);
    }

    #[test]
    fn defaults_invalid_skip_intro_seconds_to_zero() {
        assert_eq!(parse_skip_intro_seconds(None), 0.0);
        assert_eq!(parse_skip_intro_seconds(Some("".to_string())), 0.0);
        assert_eq!(parse_skip_intro_seconds(Some("invalid".to_string())), 0.0);
        assert_eq!(parse_skip_intro_seconds(Some("-5".to_string())), 0.0);
        assert_eq!(parse_skip_intro_seconds(Some("NaN".to_string())), 0.0);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeVersions {
    soia_version: String,
    mpv_version: Option<String>,
    ffmpeg_version: Option<String>,
}

fn normalize_mpv_version(value: Option<String>) -> Option<String> {
    value.map(|raw| raw.trim().to_string()).and_then(|trimmed| {
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.strip_prefix("mpv ").unwrap_or(&trimmed).to_string())
        }
    })
}

fn normalize_generic_version(value: Option<String>) -> Option<String> {
    value.map(|raw| raw.trim().to_string()).and_then(|trimmed| {
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

#[tauri::command]
pub(crate) fn get_runtime_versions(
    state: tauri::State<'_, AppState>,
) -> Result<RuntimeVersions, String> {
    let (mpv_version, ffmpeg_version) = with_mpv(&state, |mpv_guard| {
        Ok((
            mpv_guard.get_property_string("mpv-version").ok(),
            mpv_guard.get_property_string("ffmpeg-version").ok(),
        ))
    })?;

    Ok(RuntimeVersions {
        soia_version: env!("CARGO_PKG_VERSION").to_string(),
        mpv_version: normalize_mpv_version(mpv_version),
        ffmpeg_version: normalize_generic_version(ffmpeg_version),
    })
}

fn resolve_local_media_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("file://") {
        let parsed = url::Url::parse(trimmed).ok()?;
        if parsed.scheme() == "file" {
            return parsed.to_file_path().ok();
        }
        return None;
    }
    Some(PathBuf::from(trimmed))
}

#[tauri::command]
pub(crate) fn get_media_file_size(path: String) -> Result<Option<u64>, String> {
    let Some(local_path) = resolve_local_media_path(&path) else {
        return Ok(None);
    };
    if !local_path.is_file() {
        return Ok(None);
    }
    match std::fs::metadata(local_path) {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(_) => Ok(None),
    }
}

#[tauri::command]
pub(crate) fn list_local_media_siblings(path: String) -> Result<Vec<String>, String> {
    let Some(local_path) = resolve_local_media_path(&path) else {
        return Ok(Vec::new());
    };
    if !local_path.is_file() {
        return Ok(Vec::new());
    }
    let Some(parent) = local_path.parent() else {
        return Ok(Vec::new());
    };

    let mut files: Vec<PathBuf> = std::fs::read_dir(parent)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry_path| entry_path.is_file())
        .collect();

    files.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let right_name = right
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_lowercase();
        left_name.cmp(&right_name)
    });

    Ok(files
        .into_iter()
        .map(|entry_path| entry_path.to_string_lossy().into_owned())
        .collect())
}

#[tauri::command]
pub(crate) fn consume_pending_open_files(
    state: tauri::State<'_, OpenFileState>,
) -> Result<Vec<String>, String> {
    let mut pending = state.pending_paths.lock().map_err(|e| e.to_string())?;
    Ok(std::mem::take(&mut *pending))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveYoutubePlaylistPayload {
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedYoutubePlaylistEntry {
    url: String,
    title: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedYoutubePlaylist {
    playlist_title: Option<String>,
    entries: Vec<ResolvedYoutubePlaylistEntry>,
}

#[tauri::command]
pub(crate) async fn resolve_youtube_playlist(
    app: tauri::AppHandle,
    payload: ResolveYoutubePlaylistPayload,
) -> Result<ResolvedYoutubePlaylist, String> {
    let resolved = crate::mpv::resolve_ytdlp_playlist(&app, &payload.url).await?;
    Ok(ResolvedYoutubePlaylist {
        playlist_title: resolved.title,
        entries: resolved
            .entries
            .into_iter()
            .map(|entry| ResolvedYoutubePlaylistEntry {
                url: entry.url,
                title: entry.title,
            })
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParsePlaylistFilePayload {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParsePlaylistSourcePayload {
    source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParsedPlaylistEntry {
    path: String,
    title: Option<String>,
    icon: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParsedPlaylistMetadata {
    has_end_list: bool,
    playlist_type: Option<String>,
    target_duration: Option<f64>,
    has_hls_tags: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParsedPlaylistFile {
    entries: Vec<ParsedPlaylistEntry>,
    metadata: ParsedPlaylistMetadata,
}

enum PlaylistBase<'a> {
    Local(&'a Path),
    Remote(url::Url),
}

fn is_absolute_local_path(candidate: &str) -> bool {
    let path = Path::new(candidate);
    if path.is_absolute() {
        return true;
    }
    let bytes = candidate.as_bytes();
    bytes.len() >= 3
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && bytes[0].is_ascii_alphabetic()
}

fn resolve_playlist_entry_path(base: &PlaylistBase<'_>, raw: &str) -> Option<String> {
    let candidate = raw.trim();
    if candidate.is_empty() {
        return None;
    }
    if let Ok(parsed) = url::Url::parse(candidate) {
        if !parsed.scheme().is_empty() {
            return Some(candidate.to_string());
        }
    }
    if is_absolute_local_path(candidate) {
        return Some(candidate.to_string());
    }
    match base {
        PlaylistBase::Local(base_dir) => {
            Some(base_dir.join(candidate).to_string_lossy().into_owned())
        }
        PlaylistBase::Remote(base_url) => {
            base_url.join(candidate).ok().map(|item| item.to_string())
        }
    }
}

fn parse_extinf_attribute(line: &str, key: &str) -> Option<String> {
    let key_prefix = format!("{}=", key);
    let lower_line = line.to_ascii_lowercase();
    let start = lower_line.find(&key_prefix)? + key_prefix.len();
    let rest = &line[start..];
    let value = if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        &stripped[..end]
    } else {
        rest.split_whitespace().next().unwrap_or_default()
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_extinf_title(line: &str) -> Option<String> {
    let mut in_quotes = false;
    let mut comma_index = None;
    for (index, ch) in line.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
            continue;
        }
        if ch == ',' && !in_quotes {
            comma_index = Some(index);
            break;
        }
    }
    let comma_index = comma_index?;
    let title = line[(comma_index + 1)..].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn parse_extinf_icon(line: &str, base: &PlaylistBase<'_>) -> Option<String> {
    parse_extinf_attribute(line, "tvg-logo")
        .or_else(|| parse_extinf_attribute(line, "logo"))
        .and_then(|raw| resolve_playlist_entry_path(base, &raw))
}

fn parse_playlist_type(line: &str) -> Option<String> {
    let value = line.split_once(':')?.1.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_ascii_uppercase())
    }
}

fn parse_target_duration(line: &str) -> Option<f64> {
    line.split_once(':')?.1.trim().parse::<f64>().ok()
}

fn parse_m3u_playlist(content: &str, base: &PlaylistBase<'_>) -> ParsedPlaylistFile {
    let mut entries = Vec::new();
    let mut pending_title: Option<String> = None;
    let mut pending_icon: Option<String> = None;
    let mut metadata = ParsedPlaylistMetadata::default();

    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.is_empty() {
            log::debug!("playlist parse: skip empty line {}", index + 1);
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("#EXTINF:") {
            pending_title = parse_extinf_title(stripped);
            pending_icon = parse_extinf_icon(stripped, base);
            log::debug!(
                "playlist parse: line {} extinf title={:?} icon={:?}",
                index + 1,
                pending_title,
                pending_icon
            );
            continue;
        }
        if trimmed.starts_with("#EXT-X-") {
            metadata.has_hls_tags = true;
            if trimmed.eq_ignore_ascii_case("#EXT-X-ENDLIST") {
                metadata.has_end_list = true;
            } else if trimmed
                .to_ascii_uppercase()
                .starts_with("#EXT-X-PLAYLIST-TYPE:")
            {
                metadata.playlist_type = parse_playlist_type(trimmed);
            } else if trimmed
                .to_ascii_uppercase()
                .starts_with("#EXT-X-TARGETDURATION:")
            {
                metadata.target_duration = parse_target_duration(trimmed);
            }
        }
        if trimmed.starts_with('#') {
            log::debug!("playlist parse: line {} comment/metadata", index + 1);
            continue;
        }

        let Some(path) = resolve_playlist_entry_path(base, trimmed) else {
            log::debug!("playlist parse: line {} unresolved entry", index + 1);
            continue;
        };
        let title = pending_title.take();
        let icon = pending_icon.take();
        log::debug!(
            "playlist parse: line {} entry path={} title={:?} icon={:?}",
            index + 1,
            path,
            title,
            icon
        );
        entries.push(ParsedPlaylistEntry { path, title, icon });
    }

    ParsedPlaylistFile { entries, metadata }
}

fn parse_playlist_source_inner(
    app: &tauri::AppHandle,
    source: &str,
) -> Result<ParsedPlaylistFile, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err("Playlist source is empty".to_string());
    }

    if let Ok(url) = url::Url::parse(trimmed) {
        if matches!(url.scheme(), "http" | "https") {
            log::info!("playlist parse: start url={}", url);
            let client = crate::network::proxy::configure_blocking_client_builder(
                app,
                reqwest::blocking::Client::builder().timeout(Duration::from_secs(20)),
            )?
            .build()
                .map_err(|e| e.to_string())?;
            let response = client.get(url.clone()).send().map_err(|e| e.to_string())?;
            let status = response.status();
            if !status.is_success() {
                log::warn!("playlist parse: fetch failed url={} status={}", url, status);
                return Err(format!("Playlist request failed: {}", status));
            }
            let content = response.text().map_err(|e| e.to_string())?;
            log::debug!(
                "playlist parse: loaded {} bytes from {}",
                content.len(),
                url
            );
            let parsed = parse_m3u_playlist(&content, &PlaylistBase::Remote(url.clone()));
            log::info!(
                "playlist parse: done url={} entries={} has_hls_tags={}",
                url,
                parsed.entries.len(),
                parsed.metadata.has_hls_tags
            );
            return Ok(parsed);
        }
    }

    let input_path = PathBuf::from(trimmed);
    log::info!(
        "playlist parse: start path={}",
        input_path.to_string_lossy()
    );
    if !input_path.is_file() {
        log::warn!(
            "playlist parse: file not found path={}",
            input_path.to_string_lossy()
        );
        return Err("Playlist file not found".to_string());
    }
    let content = std::fs::read_to_string(&input_path).map_err(|e| e.to_string())?;
    log::debug!(
        "playlist parse: loaded {} bytes from {}",
        content.len(),
        input_path.to_string_lossy()
    );
    let base_dir = input_path.parent().unwrap_or_else(|| Path::new(""));
    let parsed = parse_m3u_playlist(&content, &PlaylistBase::Local(base_dir));
    log::info!(
        "playlist parse: done path={} entries={} has_hls_tags={}",
        input_path.to_string_lossy(),
        parsed.entries.len(),
        parsed.metadata.has_hls_tags
    );
    Ok(parsed)
}

#[tauri::command]
pub(crate) fn parse_playlist_file(
    app: tauri::AppHandle,
    payload: ParsePlaylistFilePayload,
) -> Result<ParsedPlaylistFile, String> {
    parse_playlist_source_inner(&app, &payload.path)
}

#[tauri::command]
pub(crate) fn parse_playlist_source(
    app: tauri::AppHandle,
    payload: ParsePlaylistSourcePayload,
) -> Result<ParsedPlaylistFile, String> {
    parse_playlist_source_inner(&app, &payload.source)
}

pub(crate) fn prepare_playlist_import(
    app: &tauri::AppHandle,
    source: &str,
) -> Result<crate::core::playlist_service::PreparedPlaylist, String> {
    let parsed = parse_playlist_source_inner(app, source)?;
    let name = Path::new(source)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Imported Playlist")
        .to_string();
    Ok(crate::core::playlist_service::PreparedPlaylist {
        name,
        entries: parsed.entries.into_iter().map(|entry| crate::core::playlist_service::PreparedPlaylistEntry {
            path: entry.path,
            title: entry.title,
            artwork_ref: entry.icon,
            added_at: crate::store::media_db::now_millis(),
        }).collect(),
    })
}

#[cfg(test)]
mod playlist_source_tests {
    use super::{
        is_live_hls_playlist, is_playlist_source, parse_m3u_playlist, ParsedPlaylistMetadata,
        PlaylistBase,
    };
    use std::path::Path;

    #[test]
    fn parses_local_entries_titles_icons_and_relative_paths() {
        let parsed = parse_m3u_playlist(
            "\u{feff}#EXTM3U\n#EXTINF:-1 tvg-logo=\"logos/news.png\",News, HD\nchannels/news.ts\n#EXTINF:-1,Absolute\n/exports/movie.mkv\n",
            &PlaylistBase::Local(Path::new("/media/lists")),
        );

        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].path, "/media/lists/channels/news.ts");
        assert_eq!(parsed.entries[0].title.as_deref(), Some("News, HD"));
        assert_eq!(
            parsed.entries[0].icon.as_deref(),
            Some("/media/lists/logos/news.png")
        );
        assert_eq!(parsed.entries[1].path, "/exports/movie.mkv");
        assert_eq!(parsed.entries[1].title.as_deref(), Some("Absolute"));
        assert!(!parsed.metadata.has_hls_tags);
    }

    #[test]
    fn resolves_remote_entries_and_icons_against_playlist_url() {
        let base = url::Url::parse("https://media.example/lists/main.m3u").unwrap();
        let parsed = parse_m3u_playlist(
            "#EXTM3U\n#EXTINF:-1 tvg-logo=\"../art/channel.png\",Channel\n../streams/channel.m3u8\n",
            &PlaylistBase::Remote(base),
        );

        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(
            parsed.entries[0].path,
            "https://media.example/streams/channel.m3u8"
        );
        assert_eq!(
            parsed.entries[0].icon.as_deref(),
            Some("https://media.example/art/channel.png")
        );
    }

    #[test]
    fn characterizes_live_hls_metadata() {
        let parsed = parse_m3u_playlist(
            "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:10\nsegment-10.ts\n",
            &PlaylistBase::Remote(
                url::Url::parse("https://media.example/live/index.m3u8").unwrap(),
            ),
        );

        assert!(parsed.metadata.has_hls_tags);
        assert!(!parsed.metadata.has_end_list);
        assert_eq!(parsed.metadata.playlist_type, None);
        assert_eq!(parsed.metadata.target_duration, Some(6.0));
        assert_eq!(
            parsed.entries[0].path,
            "https://media.example/live/segment-10.ts"
        );
    }

    #[test]
    fn characterizes_vod_hls_metadata() {
        let parsed = parse_m3u_playlist(
            "#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-TARGETDURATION:8\nsegment.ts\n#EXT-X-ENDLIST\n",
            &PlaylistBase::Remote(
                url::Url::parse("https://media.example/vod/index.m3u8").unwrap(),
            ),
        );

        assert!(parsed.metadata.has_hls_tags);
        assert!(parsed.metadata.has_end_list);
        assert_eq!(parsed.metadata.playlist_type.as_deref(), Some("VOD"));
        assert_eq!(parsed.metadata.target_duration, Some(8.0));
    }

    #[test]
    fn recognizes_local_and_remote_playlist_sources() {
        assert!(is_playlist_source("/media/channels.M3U"));
        assert!(is_playlist_source("https://media.example/live/list.m3u8?token=abc"));
        assert!(!is_playlist_source("https://media.example/video.mp4"));
    }

    #[test]
    fn classifies_hls_live_and_vod_from_core_metadata() {
        assert!(is_live_hls_playlist(&ParsedPlaylistMetadata {
            has_end_list: false,
            playlist_type: None,
            target_duration: Some(6.0),
            has_hls_tags: true,
        }));
        assert!(!is_live_hls_playlist(&ParsedPlaylistMetadata {
            has_end_list: true,
            playlist_type: None,
            target_duration: Some(6.0),
            has_hls_tags: true,
        }));
        assert!(!is_live_hls_playlist(&ParsedPlaylistMetadata {
            has_end_list: false,
            playlist_type: Some("VOD".to_string()),
            target_duration: Some(6.0),
            has_hls_tags: true,
        }));
    }
}
