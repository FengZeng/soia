use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;

use crate::{
    json_value_to_string, mpv_command_checked, mpv_set_option_string_checked, with_mpv, AppState,
    OpenFileState,
};

#[tauri::command]
pub(crate) fn execute_playback_command(
    state: tauri::State<'_, AppState>,
    envelope: crate::protocol::CommandEnvelopeDto,
) -> Result<crate::protocol::CommandResultDto, crate::protocol::CoreErrorDto> {
    state.playback_service.execute(&state, envelope)
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

fn publish_source_load_state(
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

fn legacy_command_envelope(command: crate::protocol::PlaybackCommandDto) -> crate::protocol::CommandEnvelopeDto {
    crate::protocol::CommandEnvelopeDto {
        command_id: uuid::Uuid::now_v7().to_string(),
        client_id: "legacy-tauri".to_string(),
        playback_session_id: None,
        command,
    }
}

fn core_error_message(error: crate::protocol::CoreErrorDto) -> String {
    match error {
        crate::protocol::CoreErrorDto::InvalidCommand { message }
        | crate::protocol::CoreErrorDto::ExecutionFailed { message } => message,
    }
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
    let request_key = payload.key_or_url.trim().to_string();
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
            match with_mpv(&state, |mpv_guard| {
                crate::core::playback_loading::load(
                    mpv_guard,
                    &prepared.playback_url,
                    &prepared.mpv_load_options,
                    load_options,
                    prepared.command_mode,
                )
            }) {
                Ok(()) => {
                    {
                        let mut current_playback_key = state
                            .current_playback_key
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        *current_playback_key = Some(source_key.clone());
                    }
                    publish_source_load_state(&app, &state, false, None, None);
                    Ok(LoadPlaybackSourceResult {
                        playback_key: Some(source_key.clone()),
                        title: prepared.title,
                        is_live_playback: prepared.is_live_playback,
                        superseded: false,
                    })
                }
                Err(error) => {
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

#[tauri::command]
pub(crate) fn cycle_pause(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let paused = state.playback_state.current().is_playing;
    state
        .playback_service
        .execute(&state, legacy_command_envelope(crate::protocol::PlaybackCommandDto::SetPaused { paused }))
        .map_err(core_error_message)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn seek_video(state: tauri::State<'_, AppState>, position: f64) -> Result<(), String> {
    state
        .playback_service
        .execute(&state, legacy_command_envelope(crate::protocol::PlaybackCommandDto::SeekAbsolute { position }))
        .map_err(core_error_message)?;
    Ok(())
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
