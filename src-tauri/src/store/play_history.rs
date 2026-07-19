use crate::store::playback_store;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayHistoryEntry {
    #[serde(default)]
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub title: String,
    pub last_position: f64,
    #[serde(default)]
    pub duration: f64,
    pub last_played_at: i64,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub is_live_playback: bool,
    #[serde(default)]
    pub external_audio_tracks: Vec<String>,
    #[serde(default)]
    pub external_sub_tracks: Vec<String>,
}

pub fn load_play_history(app: &tauri::AppHandle) -> Result<Vec<PlayHistoryEntry>, String> {
    playback_store::load_play_history(app)
}

pub fn resolve_resume_position(
    app: &tauri::AppHandle,
    path: &str,
    skip_intro_seconds: f64,
) -> Result<f64, String> {
    playback_store::resolve_resume_position(app, path, skip_intro_seconds)
}

pub fn save_play_history(
    app: &tauri::AppHandle,
    entries: Vec<PlayHistoryEntry>,
) -> Result<(), String> {
    playback_store::save_play_history(app, entries)
}

pub fn save_play_history_entry(
    app: &tauri::AppHandle,
    entry: PlayHistoryEntry,
) -> Result<(), String> {
    playback_store::save_play_history_entry(app, entry)
}

pub fn save_play_history_progress_entry(
    app: &tauri::AppHandle,
    entry: PlayHistoryEntry,
) -> Result<(), String> {
    playback_store::save_play_history_progress_entry(app, entry)
}
