use serde::Serialize;
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
    pub volume: f64,
    pub muted: bool,
    #[ts(type = "number")]
    pub playlist_position: i64,
    #[ts(type = "number")]
    pub playlist_count: i64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersionDto {
    pub version: u32,
}

pub fn export_types(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    std::fs::create_dir_all(path).map_err(|error| error.to_string())?;
    PlaybackSnapshotDto::export_all_to(path).map_err(|error| error.to_string())?;
    ProtocolVersionDto::export_all_to(path).map_err(|error| error.to_string())?;
    Ok(())
}
