use crate::store::media_db;
use crate::store::play_history::PlayHistoryEntry;
use rusqlite::{params, params_from_iter, OptionalExtension, Transaction};
use std::collections::HashMap;

const RESUME_FROM_START_THRESHOLD: f64 = 0.99;

fn resolve_resume_position_from_history(
    history: Option<(f64, f64)>,
    skip_intro_seconds: f64,
) -> f64 {
    let skip_intro_seconds = if skip_intro_seconds.is_finite() {
        skip_intro_seconds.max(0.0)
    } else {
        0.0
    };
    let resume_position = match history {
        Some((last_position, duration)) => {
            let last_position = if last_position.is_finite() {
                last_position.max(0.0)
            } else {
                0.0
            };
            if duration.is_finite()
                && duration > 0.0
                && last_position / duration > RESUME_FROM_START_THRESHOLD
            {
                0.0
            } else {
                last_position
            }
        }
        None => 0.0,
    };
    resume_position.max(skip_intro_seconds)
}

const PLAY_HISTORY_UPSERT_SQL: &str = "INSERT INTO play_history (
         id,
         path,
         title,
         last_position,
         duration,
         last_played_at,
         is_pinned,
         is_live_playback,
         external_audio,
         external_sub,
         created_at,
         updated_at,
         record_version,
         last_modified_by_device_id,
         sync_status
     )
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, 1)
     ON CONFLICT(path) DO UPDATE SET
         title = excluded.title,
         last_position = excluded.last_position,
         duration = excluded.duration,
         last_played_at = excluded.last_played_at,
         is_pinned = excluded.is_pinned,
         is_live_playback = excluded.is_live_playback,
         external_audio = excluded.external_audio,
         external_sub = excluded.external_sub,
         updated_at = excluded.updated_at,
         record_version = play_history.record_version + 1,
         last_modified_by_device_id = excluded.last_modified_by_device_id,
         sync_status = 1";

const PLAY_HISTORY_PROGRESS_UPSERT_SQL: &str = "INSERT INTO play_history (
         id,
         path,
         title,
         last_position,
         duration,
         last_played_at,
         is_pinned,
         is_live_playback,
         external_audio,
         external_sub,
         created_at,
         updated_at,
         record_version,
         last_modified_by_device_id,
         sync_status
     )
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, 1)
     ON CONFLICT(path) DO UPDATE SET
         title = CASE
             WHEN play_history.title = '' THEN excluded.title
             ELSE play_history.title
         END,
         last_position = excluded.last_position,
         duration = excluded.duration,
         last_played_at = excluded.last_played_at,
         is_live_playback = excluded.is_live_playback,
         updated_at = excluded.updated_at,
         record_version = play_history.record_version + 1,
         last_modified_by_device_id = excluded.last_modified_by_device_id,
         sync_status = 1";

const PLAYLIST_UPSERT_SQL: &str = "INSERT INTO playlist_entries (
         id,
         playlist_id,
         path,
         order_index,
         added_at,
         created_at,
         updated_at,
         record_version,
         last_modified_by_device_id,
         sync_status
     )
     VALUES (?1, 'default', ?2, ?3, ?4, ?5, ?6, 1, ?7, 1)
     ON CONFLICT(playlist_id, path) DO UPDATE SET
         order_index = excluded.order_index,
         added_at = excluded.added_at,
         updated_at = excluded.updated_at,
         record_version = playlist_entries.record_version + 1,
         last_modified_by_device_id = excluded.last_modified_by_device_id,
         sync_status = 1";

const TOMBSTONE_UPSERT_SQL: &str = "INSERT INTO sync_tombstones (
         id,
         entity_type,
         entity_id,
         payload,
         deleted_at,
         record_version,
         last_modified_by_device_id,
         sync_status
     )
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 2)
     ON CONFLICT(entity_type, entity_id) DO UPDATE SET
         payload = excluded.payload,
         deleted_at = excluded.deleted_at,
         record_version = excluded.record_version,
         last_modified_by_device_id = excluded.last_modified_by_device_id,
         sync_status = 2";

#[derive(Clone)]
struct DeleteCandidate {
    id: String,
    payload: String,
    record_version: i64,
}

#[derive(Clone)]
pub struct DefaultPlaylistEntry {
    pub path: String,
    pub added_at: i64,
}

fn serialize_external_tracks(tracks: &[String]) -> String {
    serde_json::to_string(tracks).unwrap_or_else(|_| "[]".into())
}

fn parse_external_tracks(value: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&value).unwrap_or_default()
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>, String> {
    rows.map(|row| row.map_err(|e| e.to_string())).collect()
}

fn touch_sync_state(tx: &Transaction<'_>, scope: &str, now: i64) -> Result<(), String> {
    tx.execute(
        "UPDATE sync_state
         SET updated_at = ?1
         WHERE scope = ?2",
        params![now, scope],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn delete_rows_by_ids(tx: &Transaction<'_>, table: &str, ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat("?")
        .take(ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("DELETE FROM {table} WHERE id IN ({placeholders})");
    tx.execute(&sql, params_from_iter(ids.iter()))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn write_tombstones(
    tx: &Transaction<'_>,
    entity_type: &str,
    rows: &[DeleteCandidate],
    device_id: &str,
    deleted_at: i64,
) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut stmt = tx
        .prepare(TOMBSTONE_UPSERT_SQL)
        .map_err(|e| e.to_string())?;
    for row in rows {
        let record_version = row.record_version.saturating_add(1);
        stmt.execute(params![
            format!("{entity_type}:{}", row.id),
            entity_type,
            row.id,
            row.payload,
            deleted_at,
            record_version,
            device_id,
        ])
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn select_play_history_delete_candidates(
    tx: &Transaction<'_>,
    entries: &[PlayHistoryEntry],
) -> Result<Vec<DeleteCandidate>, String> {
    if entries.is_empty() {
        let mut stmt = tx
            .prepare(
                "SELECT id, path, record_version
                 FROM play_history",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let path: String = row.get(1)?;
                let payload = serde_json::json!({ "path": path }).to_string();
                Ok(DeleteCandidate {
                    id: row.get(0)?,
                    payload,
                    record_version: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        return collect_rows(rows);
    }

    let placeholders = std::iter::repeat("?")
        .take(entries.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, path, record_version
         FROM play_history
         WHERE path NOT IN ({placeholders})"
    );
    let mut stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params_from_iter(entries.iter().map(|entry| &entry.path)),
            |row| {
                let path: String = row.get(1)?;
                let payload = serde_json::json!({ "path": path }).to_string();
                Ok(DeleteCandidate {
                    id: row.get(0)?,
                    payload,
                    record_version: row.get(2)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;
    collect_rows(rows)
}

fn select_playlist_delete_candidates(
    tx: &Transaction<'_>,
    entries: &[DefaultPlaylistEntry],
) -> Result<Vec<DeleteCandidate>, String> {
    if entries.is_empty() {
        let mut stmt = tx
            .prepare(
                "SELECT id, path, playlist_id, record_version
                 FROM playlist_entries
                 WHERE playlist_id = 'default'",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let path: String = row.get(1)?;
                let playlist_id: String = row.get(2)?;
                let payload = serde_json::json!({
                    "path": path,
                    "playlistId": playlist_id,
                })
                .to_string();
                Ok(DeleteCandidate {
                    id: row.get(0)?,
                    payload,
                    record_version: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        return collect_rows(rows);
    }

    let placeholders = std::iter::repeat("?")
        .take(entries.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, path, playlist_id, record_version
         FROM playlist_entries
         WHERE playlist_id = 'default'
           AND path NOT IN ({placeholders})"
    );
    let mut stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params_from_iter(entries.iter().map(|entry| &entry.path)),
            |row| {
                let path: String = row.get(1)?;
                let playlist_id: String = row.get(2)?;
                let payload = serde_json::json!({
                    "path": path,
                    "playlistId": playlist_id,
                })
                .to_string();
                Ok(DeleteCandidate {
                    id: row.get(0)?,
                    payload,
                    record_version: row.get(3)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;
    collect_rows(rows)
}

fn should_stage_playlist_reorder(
    tx: &Transaction<'_>,
    entries: &[DefaultPlaylistEntry],
) -> Result<bool, String> {
    let mut stmt = tx
        .prepare(
            "SELECT path, order_index
             FROM playlist_entries
             WHERE playlist_id = 'default'",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let path: String = row.get(0)?;
            let order_index: i64 = row.get(1)?;
            Ok((path, order_index))
        })
        .map_err(|e| e.to_string())?;

    let current_rows = collect_rows(rows)?;
    let mut current_path_by_order: HashMap<i64, String> = HashMap::new();
    let mut current_order_by_path: HashMap<String, i64> = HashMap::new();
    for (path, order_index) in current_rows {
        current_path_by_order.insert(order_index, path.clone());
        current_order_by_path.insert(path, order_index);
    }

    for (order_index, entry) in entries.iter().enumerate() {
        let order_index = order_index as i64;
        if let Some(path_at_order) = current_path_by_order.get(&order_index) {
            if path_at_order != &entry.path {
                return Ok(true);
            }
        }
        if let Some(existing_order) = current_order_by_path.get(&entry.path) {
            if *existing_order != order_index {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn trim_play_history(tx: &Transaction<'_>, device_id: &str, now: i64) -> Result<(), String> {
    let mut stmt = tx
        .prepare(
            "SELECT id, path, record_version
             FROM play_history
             WHERE id NOT IN (
                 SELECT id
                 FROM play_history
                 ORDER BY is_pinned DESC, last_played_at DESC
                 LIMIT ?1
             )",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([media_db::MAX_PLAY_HISTORY], |row| {
            let path: String = row.get(1)?;
            let payload = serde_json::json!({ "path": path }).to_string();
            Ok(DeleteCandidate {
                id: row.get(0)?,
                payload,
                record_version: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let candidates = collect_rows(rows)?;

    write_tombstones(tx, "play_history", &candidates, device_id, now)?;
    let ids = candidates
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    delete_rows_by_ids(tx, "play_history", &ids)?;
    Ok(())
}

fn upsert_play_history_entries(
    tx: &Transaction<'_>,
    entries: &[PlayHistoryEntry],
    device_id: &str,
    now: i64,
) -> Result<(), String> {
    let mut stmt = tx
        .prepare(PLAY_HISTORY_UPSERT_SQL)
        .map_err(|e| e.to_string())?;
    for entry in entries {
        stmt.execute(params![
            media_db::normalize_uuid_or_new(&entry.id),
            entry.path,
            entry.title,
            entry.last_position,
            entry.duration,
            entry.last_played_at,
            entry.is_pinned,
            entry.is_live_playback,
            serialize_external_tracks(&entry.external_audio_tracks),
            serialize_external_tracks(&entry.external_sub_tracks),
            now,
            now,
            device_id,
        ])
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn upsert_play_history_progress_entry(
    tx: &Transaction<'_>,
    entry: &PlayHistoryEntry,
    device_id: &str,
    now: i64,
) -> Result<(), String> {
    tx.execute(
        PLAY_HISTORY_PROGRESS_UPSERT_SQL,
        params![
            media_db::normalize_uuid_or_new(&entry.id),
            entry.path,
            entry.title,
            entry.last_position,
            entry.duration,
            entry.last_played_at,
            entry.is_pinned,
            entry.is_live_playback,
            serialize_external_tracks(&entry.external_audio_tracks),
            serialize_external_tracks(&entry.external_sub_tracks),
            now,
            now,
            device_id,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn delete_play_history_except(
    tx: &Transaction<'_>,
    entries: &[PlayHistoryEntry],
    device_id: &str,
    now: i64,
) -> Result<(), String> {
    let candidates = select_play_history_delete_candidates(tx, entries)?;
    if candidates.is_empty() {
        return Ok(());
    }

    write_tombstones(tx, "play_history", &candidates, device_id, now)?;
    let ids = candidates
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    delete_rows_by_ids(tx, "play_history", &ids)?;
    Ok(())
}

fn delete_playlist_except(
    tx: &Transaction<'_>,
    entries: &[DefaultPlaylistEntry],
    device_id: &str,
    now: i64,
) -> Result<(), String> {
    let candidates = select_playlist_delete_candidates(tx, entries)?;
    if candidates.is_empty() {
        return Ok(());
    }

    write_tombstones(tx, "playlist_entries", &candidates, device_id, now)?;
    let ids = candidates
        .into_iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    delete_rows_by_ids(tx, "playlist_entries", &ids)?;
    Ok(())
}

pub fn load_playlist(app: &tauri::AppHandle) -> Result<Vec<DefaultPlaylistEntry>, String> {
    let conn = media_db::open_db(app)?;
    let mut stmt = conn
        .prepare(
            "SELECT path, added_at
             FROM playlist_entries
             WHERE playlist_id = 'default'
             ORDER BY order_index ASC, added_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DefaultPlaylistEntry {
                path: row.get(0)?,
                added_at: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    collect_rows(rows)
}

pub fn save_playlist(
    app: &tauri::AppHandle,
    entries: Vec<DefaultPlaylistEntry>,
) -> Result<(), String> {
    let mut conn = media_db::open_db(app)?;
    let device_id = media_db::local_device_id(&conn)?;
    let now = media_db::now_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    if should_stage_playlist_reorder(&tx, &entries)? {
        // Move existing order values out of the target range first so
        // reordering does not violate UNIQUE(playlist_id, order_index).
        tx.execute(
            "UPDATE playlist_entries
             SET order_index = order_index + 1000000
             WHERE playlist_id = 'default'",
            [],
        )
        .map_err(|e| e.to_string())?;
    }

    {
        let mut stmt = tx.prepare(PLAYLIST_UPSERT_SQL).map_err(|e| e.to_string())?;
        for (order_index, entry) in entries.iter().enumerate() {
            stmt.execute(params![
                media_db::new_uuid(),
                entry.path,
                order_index as i64,
                entry.added_at,
                now,
                now,
                &device_id,
            ])
            .map_err(|e| e.to_string())?;
        }
    }

    delete_playlist_except(&tx, &entries, &device_id, now)?;
    touch_sync_state(&tx, "playlist_entries", now)?;
    touch_sync_state(&tx, "tombstones", now)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_play_history(app: &tauri::AppHandle) -> Result<Vec<PlayHistoryEntry>, String> {
    let conn = media_db::open_db(app)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, path, title, last_position, duration, last_played_at, is_pinned, is_live_playback, external_audio, external_sub
             FROM play_history
             ORDER BY is_pinned DESC, last_played_at DESC
             LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([media_db::MAX_PLAY_HISTORY], |row| {
            Ok(PlayHistoryEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
                last_position: row.get(3)?,
                duration: row.get(4)?,
                last_played_at: row.get(5)?,
                is_pinned: row.get(6)?,
                is_live_playback: row.get(7)?,
                external_audio_tracks: parse_external_tracks(row.get(8)?),
                external_sub_tracks: parse_external_tracks(row.get(9)?),
            })
        })
        .map_err(|e| e.to_string())?;
    collect_rows(rows)
}

pub fn resolve_resume_position(
    app: &tauri::AppHandle,
    path: &str,
    skip_intro_seconds: f64,
) -> Result<f64, String> {
    let conn = media_db::open_db(app)?;
    let history = conn
        .query_row(
            "SELECT last_position, duration FROM play_history WHERE path = ?1",
            [path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(resolve_resume_position_from_history(
        history,
        skip_intro_seconds,
    ))
}

#[cfg(test)]
mod resume_tests {
    use super::resolve_resume_position_from_history;

    #[test]
    fn applies_skip_intro_with_or_without_history() {
        assert_eq!(resolve_resume_position_from_history(None, 12.0), 12.0);
        assert_eq!(
            resolve_resume_position_from_history(Some((0.0, 120.0)), 12.0),
            12.0,
        );
        assert_eq!(
            resolve_resume_position_from_history(Some((5.0, 120.0)), 12.0),
            12.0,
        );
        assert_eq!(
            resolve_resume_position_from_history(Some((30.0, 120.0)), 12.0),
            30.0,
        );
    }

    #[test]
    fn restarts_nearly_completed_media() {
        assert_eq!(
            resolve_resume_position_from_history(Some((99.5, 100.0)), 0.0),
            0.0,
        );
        assert_eq!(
            resolve_resume_position_from_history(Some((99.0, 100.0)), 0.0),
            99.0,
        );
        assert_eq!(
            resolve_resume_position_from_history(Some((99.5, 100.0)), 12.0),
            12.0,
        );
    }
}

pub fn save_play_history(
    app: &tauri::AppHandle,
    entries: Vec<PlayHistoryEntry>,
) -> Result<(), String> {
    let mut conn = media_db::open_db(app)?;
    let device_id = media_db::local_device_id(&conn)?;
    let now = media_db::now_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    upsert_play_history_entries(&tx, &entries, &device_id, now)?;
    delete_play_history_except(&tx, &entries, &device_id, now)?;
    trim_play_history(&tx, &device_id, now)?;

    touch_sync_state(&tx, "play_history", now)?;
    touch_sync_state(&tx, "tombstones", now)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_play_history_entry(
    app: &tauri::AppHandle,
    entry: PlayHistoryEntry,
) -> Result<(), String> {
    let mut conn = media_db::open_db(app)?;
    let device_id = media_db::local_device_id(&conn)?;
    let now = media_db::now_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    upsert_play_history_entries(&tx, &[entry], &device_id, now)?;
    trim_play_history(&tx, &device_id, now)?;

    touch_sync_state(&tx, "play_history", now)?;
    touch_sync_state(&tx, "tombstones", now)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_play_history_progress_entry(
    app: &tauri::AppHandle,
    entry: PlayHistoryEntry,
) -> Result<(), String> {
    let mut conn = media_db::open_db(app)?;
    let device_id = media_db::local_device_id(&conn)?;
    let now = media_db::now_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    upsert_play_history_progress_entry(&tx, &entry, &device_id, now)?;
    trim_play_history(&tx, &device_id, now)?;

    touch_sync_state(&tx, "play_history", now)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}
