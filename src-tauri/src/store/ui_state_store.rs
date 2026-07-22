use crate::store::{json_io, playback_store, storage_paths};
use log::info;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const STATE_FILE_NAME: &str = "state.json";
const LEGACY_STATE_FILE_NAME: &str = "ui_state.json";

fn ui_state_cache() -> &'static Mutex<Option<UiState>> {
    static UI_STATE_CACHE: OnceLock<Mutex<Option<UiState>>> = OnceLock::new();
    UI_STATE_CACHE.get_or_init(|| Mutex::new(None))
}

fn cached_ui_state() -> Option<UiState> {
    ui_state_cache().lock().ok().and_then(|cache| cache.clone())
}

fn store_ui_state_cache(state: &UiState) {
    if let Ok(mut cache) = ui_state_cache().lock() {
        *cache = Some(strip_playlist(state));
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    #[serde(default)]
    pub active_panel: Option<String>,
    #[serde(default)]
    pub window_state: Option<ManualWindowState>,
    #[serde(default)]
    pub network: Option<NetworkState>,
    #[serde(default)]
    pub settings: Option<SettingsState>,
    #[serde(default)]
    pub rendering: Option<RenderingState>,
    #[serde(default)]
    pub playback_adjustments: Option<PlaybackAdjustmentsState>,
    #[serde(default)]
    pub playback: Option<PlaybackState>,
    #[serde(default)]
    pub subtitle_appearance: Option<SubtitleAppearanceState>,
    #[serde(default)]
    pub playlist: Option<Vec<PlaylistEntry>>,
    #[serde(default)]
    pub playlists: Option<Vec<Playlist>>,
    #[serde(default)]
    pub active_playlist_id: Option<String>,
    #[serde(default)]
    pub playlist_loop_mode: Option<String>,
    #[serde(default)]
    pub playlist_sort_mode: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            active_panel: Some("home".into()),
            window_state: None,
            network: None,
            settings: None,
            rendering: None,
            playback_adjustments: None,
            playback: None,
            subtitle_appearance: None,
            playlist: None,
            playlists: None,
            active_playlist_id: None,
            playlist_loop_mode: None,
            playlist_sort_mode: None,
        }
    }
}

impl UiState {
    fn merge(self, incoming: UiState) -> UiState {
        UiState {
            active_panel: incoming.active_panel.or(self.active_panel),
            window_state: incoming.window_state.or(self.window_state),
            network: incoming.network.or(self.network),
            settings: incoming.settings.or(self.settings),
            rendering: incoming.rendering.or(self.rendering),
            playback_adjustments: incoming
                .playback_adjustments
                .or(self.playback_adjustments),
            playback: incoming.playback.or(self.playback),
            subtitle_appearance: incoming.subtitle_appearance.or(self.subtitle_appearance),
            playlist: incoming.playlist.or(self.playlist),
            playlists: incoming.playlists.or(self.playlists),
            active_playlist_id: incoming.active_playlist_id.or(self.active_playlist_id),
            playlist_loop_mode: incoming.playlist_loop_mode.or(self.playlist_loop_mode),
            playlist_sort_mode: incoming.playlist_sort_mode.or(self.playlist_sort_mode),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManualWindowState {
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub is_maximized: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackAdjustmentsState {
    #[serde(default)]
    pub global_color_adjustments_enabled: Option<bool>,
    #[serde(default)]
    pub global_color_adjustments: Option<ColorAdjustmentsState>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    #[serde(default)]
    pub volume: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColorAdjustmentsState {
    #[serde(default)]
    pub brightness: Option<f64>,
    #[serde(default)]
    pub contrast: Option<f64>,
    #[serde(default)]
    pub saturation: Option<f64>,
    #[serde(default)]
    pub gamma: Option<f64>,
    #[serde(default)]
    pub hue: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleAppearanceState {
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub font_size: Option<f64>,
    #[serde(default)]
    pub font_color: Option<String>,
    #[serde(default)]
    pub primary_sub_pos: Option<f64>,
    #[serde(default)]
    pub secondary_sub_pos: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub entries: Vec<PlaylistEntry>,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub path: String,
    #[serde(default)]
    pub title: Option<String>,
    pub added_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkState {
    #[serde(default)]
    pub selected_connection: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsState {
    #[serde(default)]
    pub groups: Option<Vec<SettingsGroup>>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RenderingState {
    #[serde(default)]
    pub rendering_mode: Option<String>,
    #[serde(default)]
    pub selected_shader_files: Option<Vec<String>>,
    #[serde(default)]
    pub active_shader_files: Option<Vec<String>>,
    #[serde(default)]
    pub normal_active_shader_files: Option<Vec<String>>,
    #[serde(default, alias = "animeActiveShaderFiles")]
    pub anime_mode_active_shader_files: Option<Vec<String>>,
    #[serde(default, alias = "animeAutoShaderEnabled")]
    pub anime_mode_enabled: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsGroup {
    pub title: String,
    pub items: Vec<SettingsItem>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsItem {
    pub label: String,
    pub value: String,
}

fn ui_state_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = storage_paths::app_data_dir(app)?;
    Ok(data_dir.join(STATE_FILE_NAME))
}

fn legacy_ui_state_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = storage_paths::app_data_dir(app)?;
    Ok(data_dir.join(LEGACY_STATE_FILE_NAME))
}

fn strip_playlist(state: &UiState) -> UiState {
    let mut stripped = state.clone();
    stripped.playlist = None;
    stripped
}

fn from_default_playlist_entries(
    entries: Vec<playback_store::DefaultPlaylistEntry>,
) -> Vec<PlaylistEntry> {
    entries
        .into_iter()
        .map(|entry| PlaylistEntry {
            path: entry.path,
            title: None,
            added_at: entry.added_at,
        })
        .collect()
}

fn into_default_playlist_entries(
    entries: Vec<PlaylistEntry>,
) -> Vec<playback_store::DefaultPlaylistEntry> {
    entries
        .into_iter()
        .map(|entry| playback_store::DefaultPlaylistEntry {
            path: entry.path,
            added_at: entry.added_at,
        })
        .collect()
}

fn load_state_from_disk(path: &Path, legacy_path: &Path) -> Result<UiState, String> {
    if let Some(state) = cached_ui_state() {
        return Ok(state);
    }

    if path.exists() {
        info!("Loading UI state from {}", path.display());
        let state: UiState = json_io::read_json_or_default(path)?;
        store_ui_state_cache(&state);
        return Ok(state);
    }

    if legacy_path.exists() {
        info!("Loading UI state from {}", legacy_path.display());
        let legacy_state: UiState = json_io::read_json_or_default(legacy_path)?;
        json_io::write_json(path, &strip_playlist(&legacy_state))?;
        store_ui_state_cache(&legacy_state);
        return Ok(legacy_state);
    }

    let default_state = UiState::default();
    store_ui_state_cache(&default_state);
    Ok(default_state)
}

pub fn load_ui_state(app: &tauri::AppHandle) -> Result<UiState, String> {
    let path = ui_state_file_path(app)?;
    let legacy_path = legacy_ui_state_file_path(app)?;
    let mut state = load_state_from_disk(&path, &legacy_path)?;

    let playlist = playback_store::load_playlist(app)?;
    let legacy_playlist = state.playlist.take();
    if legacy_playlist.is_some() && path.exists() {
        json_io::write_json(&path, &state)?;
    }
    store_ui_state_cache(&state);
    state.playlist = Some(from_default_playlist_entries(playlist));
    Ok(state)
}

pub fn load_setting_value(app: &tauri::AppHandle, label: &str) -> Result<Option<String>, String> {
    let path = ui_state_file_path(app)?;
    let legacy_path = legacy_ui_state_file_path(app)?;
    let state = load_state_from_disk(&path, &legacy_path)?;

    let value = state
        .settings
        .and_then(|settings| settings.groups)
        .and_then(|groups| {
            groups
                .into_iter()
                .flat_map(|group| group.items.into_iter())
                .find(|item| item.label == label)
                .map(|item| item.value)
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok(value)
}

const YTDL_COOKIES_FROM_BROWSER_LABEL: &str = "SOIA_YTDL_COOKIES_FROM_BROWSER";

pub fn load_ytdl_cookies_from_browser(app: &tauri::AppHandle) -> Option<String> {
    load_setting_value(app, YTDL_COOKIES_FROM_BROWSER_LABEL)
        .ok()
        .flatten()
        .filter(|v| v != "Off")
}

const YTDL_MAX_RESOLUTION_LABEL: &str = "SOIA_YTDL_MAX_RESOLUTION";

pub fn load_ytdl_max_height(app: &tauri::AppHandle) -> u32 {
    load_setting_value(app, YTDL_MAX_RESOLUTION_LABEL)
        .ok()
        .flatten()
        .and_then(|v| {
            v.trim_end_matches('p')
                .parse::<u32>()
                .ok()
                .filter(|&h| h > 0)
        })
        .unwrap_or(1080)
}

pub fn save_ui_state(app: &tauri::AppHandle, state: UiState) -> Result<(), String> {
    let path = ui_state_file_path(app)?;
    let legacy_path = legacy_ui_state_file_path(app)?;
    let mut state = state;
    if let Some(playlist) = state.playlist.take() {
        playback_store::save_playlist(app, into_default_playlist_entries(playlist))?;
    }
    let existing = load_state_from_disk(&path, &legacy_path)?;
    let merged = existing.merge(state);
    json_io::write_json(&path, &merged)?;
    store_ui_state_cache(&merged);
    Ok(())
}

pub fn reset_ui_state(app: &tauri::AppHandle) -> Result<(), String> {
    let path = ui_state_file_path(app)?;
    let legacy_path = legacy_ui_state_file_path(app)?;

    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    if legacy_path.exists() {
        fs::remove_file(&legacy_path).map_err(|e| e.to_string())?;
    }

    if let Ok(mut cache) = ui_state_cache().lock() {
        *cache = Some(UiState::default());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_entry_title_survives_ui_state_json_round_trip() {
        let state = UiState {
            playlists: Some(vec![Playlist {
                id: "pl_1".to_string(),
                name: "IPTV".to_string(),
                entries: vec![PlaylistEntry {
                    path: "https://example.test/live.m3u8".to_string(),
                    title: Some("News Channel".to_string()),
                    added_at: 123,
                }],
                created_at: 100,
            }]),
            ..UiState::default()
        };

        let json = serde_json::to_string(&state).expect("serialize ui state");
        let restored: UiState = serde_json::from_str(&json).expect("deserialize ui state");

        let title = restored
            .playlists
            .expect("playlists")
            .remove(0)
            .entries
            .remove(0)
            .title;
        assert_eq!(title.as_deref(), Some("News Channel"));
    }

    #[test]
    fn playlist_entry_title_defaults_for_legacy_ui_state_json() {
        let json = r#"{
            "playlists": [{
                "id": "pl_1",
                "name": "Legacy",
                "entries": [{
                    "path": "/media/legacy.mp4",
                    "addedAt": 123
                }],
                "createdAt": 100
            }]
        }"#;

        let restored: UiState = serde_json::from_str(json).expect("deserialize ui state");
        let title = restored
            .playlists
            .expect("playlists")
            .remove(0)
            .entries
            .remove(0)
            .title;
        assert_eq!(title, None);
    }
}
