use crate::protocol::PlaybackSnapshotDto;
use crate::store::media_db;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TrackPreferences {
    pub selected_audio_track_id: Option<i64>,
    pub selected_subtitle_track_id: Option<i64>,
    pub selected_audio_track_path: Option<String>,
    pub selected_subtitle_track_path: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TrackKind {
    Audio,
    Subtitle,
}

impl TrackKind {
    pub(crate) fn mpv_type(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Subtitle => "sub",
        }
    }

    pub(crate) fn property(self) -> &'static str {
        match self {
            Self::Audio => "aid",
            Self::Subtitle => "sid",
        }
    }

    fn columns(self) -> (&'static str, &'static str) {
        match self {
            Self::Audio => ("selected_audio_track_id", "selected_audio_track_path"),
            Self::Subtitle => ("selected_subtitle_track_id", "selected_subtitle_track_path"),
        }
    }
}

pub(crate) fn load(app: &tauri::AppHandle, path: &str) -> Result<TrackPreferences, String> {
    load_from_connection(&media_db::open_db(app)?, path)
}

fn load_from_connection(conn: &Connection, path: &str) -> Result<TrackPreferences, String> {
    conn.query_row(
        "SELECT selected_audio_track_id, selected_subtitle_track_id,
                selected_audio_track_path, selected_subtitle_track_path
         FROM play_history WHERE path = ?1",
        [path],
        |row| Ok(TrackPreferences {
            selected_audio_track_id: row.get(0)?,
            selected_subtitle_track_id: row.get(1)?,
            selected_audio_track_path: row.get(2)?,
            selected_subtitle_track_path: row.get(3)?,
        }),
    )
    .optional()
    .map(|value| value.unwrap_or_default())
    .map_err(|e| e.to_string())
}

pub(crate) fn save(
    app: &tauri::AppHandle,
    snapshot: &PlaybackSnapshotDto,
    kind: TrackKind,
    track_id: i64,
    external_path: Option<&str>,
) -> Result<(), String> {
    let mut conn = media_db::open_db(app)?;
    let device_id = media_db::local_device_id(&conn)?;
    save_to_connection(&mut conn, snapshot, kind, track_id, external_path, &device_id)
}

pub(super) fn save_to_connection(
    conn: &mut Connection,
    snapshot: &PlaybackSnapshotDto,
    kind: TrackKind,
    track_id: i64,
    external_path: Option<&str>,
    device_id: &str,
) -> Result<(), String> {
    let path = snapshot.playback_key.as_deref().ok_or("no current playback key")?;
    if track_id < 0 || (kind == TrackKind::Audio && track_id == 0) {
        return Err("invalid saved track ID".into());
    }
    let (id_column, path_column) = kind.columns();
    let now = media_db::now_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    // Ordinary history/progress upserts omit these columns, so stale UI snapshots
    // cannot erase a selection made through Desktop or WebSocket controls.
    tx.execute(
        &format!("INSERT INTO play_history (
             id, path, title, last_position, duration, last_played_at,
             created_at, updated_at, last_modified_by_device_id, sync_status,
             {id_column}, {path_column}
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6, ?7, 1, ?8, ?9)
         ON CONFLICT(path) DO UPDATE SET
             {id_column} = excluded.{id_column}, {path_column} = excluded.{path_column},
             updated_at = excluded.updated_at, record_version = play_history.record_version + 1,
             last_modified_by_device_id = excluded.last_modified_by_device_id, sync_status = 1"),
        params![media_db::new_uuid(), path, snapshot.title.as_deref().unwrap_or(""),
            snapshot.position.max(0.0), snapshot.duration.max(0.0), now, device_id, track_id,
            external_path.filter(|value| !value.is_empty())],
    ).map_err(|e| e.to_string())?;
    tx.execute("UPDATE sync_state SET updated_at = ?1 WHERE scope = 'play_history'", [now])
        .map_err(|e| e.to_string())?;
    crate::store::playback_store::trim_play_history(&tx, device_id, now)?;
    tx.execute("UPDATE sync_state SET updated_at = ?1 WHERE scope = 'tombstones'", [now])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        media_db::ensure_schema(&mut conn).unwrap();
        conn
    }

    fn snapshot(path: &str) -> PlaybackSnapshotDto {
        PlaybackSnapshotDto {
            playback_key: Some(path.into()),
            position: 42.0,
            duration: 120.0,
            title: Some("Example".into()),
            ..Default::default()
        }
    }

    #[test]
    fn stores_audio_subtitle_and_off_independently_for_each_video() {
        let mut conn = database();
        assert_eq!(load_from_connection(&conn, "video-a").unwrap(), TrackPreferences::default());
        save_to_connection(&mut conn, &snapshot("video-a"), TrackKind::Audio, 2, None, "device").unwrap();
        save_to_connection(&mut conn, &snapshot("video-a"), TrackKind::Subtitle, 3, None, "device").unwrap();
        save_to_connection(&mut conn, &snapshot("video-b"), TrackKind::Subtitle, 0, None, "device").unwrap();
        let a = load_from_connection(&conn, "video-a").unwrap();
        assert_eq!(a.selected_audio_track_id, Some(2));
        assert_eq!(a.selected_subtitle_track_id, Some(3));
        assert_eq!(load_from_connection(&conn, "video-b").unwrap().selected_subtitle_track_id, Some(0));
    }

    #[test]
    fn stores_external_identity_and_clears_it_when_switching_back_to_embedded() {
        let mut conn = database();
        save_to_connection(&mut conn, &snapshot("video"), TrackKind::Subtitle, 4, Some("/subs/a.ass"), "device").unwrap();
        assert_eq!(load_from_connection(&conn, "video").unwrap().selected_subtitle_track_path.as_deref(), Some("/subs/a.ass"));
        save_to_connection(&mut conn, &snapshot("video"), TrackKind::Subtitle, 2, None, "device").unwrap();
        assert_eq!(load_from_connection(&conn, "video").unwrap().selected_subtitle_track_path, None);
        conn.execute("DELETE FROM play_history WHERE path = 'video'", []).unwrap();
        assert_eq!(load_from_connection(&conn, "video").unwrap(), TrackPreferences::default());
    }

    #[test]
    fn selection_updates_preserve_progress_and_pinned_history() {
        let mut conn = database();
        save_to_connection(&mut conn, &snapshot("video"), TrackKind::Audio, 2, None, "device").unwrap();
        conn.execute("UPDATE play_history SET last_position = 99, is_pinned = 1, external_audio = '[\"dub.mka\"]'", []).unwrap();
        save_to_connection(&mut conn, &snapshot("video"), TrackKind::Subtitle, 0, None, "device").unwrap();
        let history: (f64, bool, String) = conn.query_row(
            "SELECT last_position, is_pinned, external_audio FROM play_history", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(history, (99.0, true, "[\"dub.mka\"]".into()));
    }

    #[test]
    fn invalid_choices_do_not_create_history() {
        let mut conn = database();
        assert!(save_to_connection(&mut conn, &snapshot("video"), TrackKind::Audio, 0, None, "device").is_err());
        assert!(save_to_connection(&mut conn, &snapshot("video"), TrackKind::Subtitle, -1, None, "device").is_err());
        assert_eq!(load_from_connection(&conn, "video").unwrap(), TrackPreferences::default());
    }

    #[test]
    fn network_track_preferences_survive_new_proxy_ports_tokens_and_track_ids() {
        use crate::core::track_selection::TrackSelectionRuntime;
        use crate::protocol::MediaTrackDto;

        for source in [
            crate::playback_source::create_webdav_playback_key("webdav-1", "/shows/episode.zh.ass"),
            crate::playback_source::create_smb_playback_key("smb-1", "/shows/episode.zh.ass"),
        ] {
            let mut conn = database();
            let old_url = "http://127.0.0.1:31000/stream/old-token";
            let new_url = "http://127.0.0.1:42000/stream/new-token";
            let runtime = TrackSelectionRuntime::default();
            runtime.begin(1, "video".into(), TrackPreferences::default(), false);
            assert!(runtime.register_external_source(1, TrackKind::Subtitle, &source, old_url));
            let saved = runtime.external_source("video", TrackKind::Subtitle, old_url).unwrap();
            assert_eq!(saved, source);
            save_to_connection(&mut conn, &snapshot("video"), TrackKind::Subtitle, 3, Some(&saved), "device").unwrap();

            let preferences = load_from_connection(&conn, "video").unwrap();
            assert_eq!(preferences.selected_subtitle_track_path.as_deref(), Some(source.as_str()));
            let reopened = TrackSelectionRuntime::default();
            reopened.begin(2, "video".into(), preferences, false);
            assert!(reopened.register_external_source(2, TrackKind::Subtitle, &source, new_url));
            let track = MediaTrackDto {
                id: 7, track_type: "sub".into(), external: Some(true),
                external_filename: Some(new_url.into()), ..Default::default()
            };
            let mut selected = Vec::new();
            reopened.restore(2, &[track], |kind, id| { selected.push((kind, id)); true });
            assert_eq!(selected, vec![(TrackKind::Subtitle, 7)]);
            assert_eq!(reopened.external_source("video", TrackKind::Subtitle, new_url).unwrap(), source);
        }
    }
}