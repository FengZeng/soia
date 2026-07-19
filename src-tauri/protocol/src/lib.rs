use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

pub const PROTOCOL_VERSION: u32 = 1;

/// Stable, transport-safe playback state sent to Tauri and WebSocket clients.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshotDto {
    pub protocol_version: u32,
    #[ts(type = "number")]
    pub revision: u64,
    pub title: Option<String>,
    pub duration: f64,
    pub position: f64,
    pub buffered_position: f64,
    pub is_playing: bool,
    pub is_buffering: bool,
    pub source_loading: bool,
    pub source_loading_key: Option<String>,
    pub source_load_error: Option<String>,
    pub speed: f64,
    pub volume: f64,
    pub muted: bool,
    #[ts(type = "number")]
    pub playlist_position: i64,
    #[ts(type = "number")]
    pub playlist_count: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersionDto {
    pub version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlaybackCommandDto {
    SetPaused { paused: bool },
    SeekAbsolute { position: f64 },
    SeekRelative { seconds: f64 },
    SetVolume { volume: f64 },
    SetMuted { muted: bool },
    SetSpeed { speed: f64 },
    Stop,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CommandEnvelopeDto {
    pub command_id: String,
    pub client_id: String,
    pub playback_session_id: Option<String>,
    pub command: PlaybackCommandDto,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CommandResultDto {
    pub command_id: String,
    #[ts(type = "number")]
    pub applied_snapshot_revision: u64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CoreErrorDto {
    InvalidCommand { message: String },
    ExecutionFailed { message: String },
}

pub fn export_types(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    std::fs::create_dir_all(path).map_err(|error| error.to_string())?;
    PlaybackSnapshotDto::export_all_to(path).map_err(|error| error.to_string())?;
    ProtocolVersionDto::export_all_to(path).map_err(|error| error.to_string())?;
    PlaybackCommandDto::export_all_to(path).map_err(|error| error.to_string())?;
    CommandEnvelopeDto::export_all_to(path).map_err(|error| error.to_string())?;
    CommandResultDto::export_all_to(path).map_err(|error| error.to_string())?;
    CoreErrorDto::export_all_to(path).map_err(|error| error.to_string())?;
    Ok(())
}
