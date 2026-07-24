use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

pub const PROTOCOL_VERSION: u32 = 2;

/// Stable, transport-safe playback state sent to Tauri and WebSocket clients.
#[derive(Clone, Debug, Default, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshotDto {
    pub protocol_version: u32,
    #[ts(type = "number")]
    pub revision: u64,
    pub playback_session_id: Option<String>,
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
    Previous,
    Next,
    PlaySource {
        key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
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
    NavigationFailed { message: String },
    StalePlaybackSession {
        message: String,
        #[serde(rename = "requestedPlaybackSessionId")]
        requested_playback_session_id: Option<String>,
        #[serde(rename = "currentPlaybackSessionId")]
        current_playback_session_id: Option<String>,
    },
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

#[cfg(test)]
mod tests {
    use super::{CoreErrorDto, PlaybackSnapshotDto};

    #[test]
    fn serializes_playback_session_fields_in_camel_case() {
        let snapshot = PlaybackSnapshotDto {
            playback_session_id: Some("session-b".to_string()),
            ..PlaybackSnapshotDto::default()
        };
        let snapshot_json = serde_json::to_value(snapshot).unwrap();
        assert_eq!(snapshot_json["playbackSessionId"], "session-b");

        let error = CoreErrorDto::StalePlaybackSession {
            message: "playback session has changed".to_string(),
            requested_playback_session_id: Some("session-a".to_string()),
            current_playback_session_id: Some("session-b".to_string()),
        };
        let error_json = serde_json::to_value(error).unwrap();
        assert_eq!(error_json["type"], "stalePlaybackSession");
        assert_eq!(error_json["requestedPlaybackSessionId"], "session-a");
        assert_eq!(error_json["currentPlaybackSessionId"], "session-b");
    }
}
