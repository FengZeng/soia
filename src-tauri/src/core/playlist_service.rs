use crate::store::media_db;
use crate::protocol::{PlaylistLoopModeDto, PlaylistSnapshotDto, PlaylistSortModeDto, PlaylistSummaryDto};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::HashSet;
use std::sync::Mutex;

pub(crate) const FAVORITES_PLAYLIST_ID: &str = "favorites";
const MAX_ENTRY_PAGE_SIZE: u32 = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub entry_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub order_index: i64,
    pub revision: i64,
    pub is_protected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaylistEntry {
    pub id: String,
    pub path: String,
    pub title: Option<String>,
    pub artwork_ref: Option<String>,
    pub added_at: i64,
    pub order_index: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaylistEntryPage {
    pub entries: Vec<PlaylistEntry>,
    pub total: i64,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaylistLoopMode {
    List,
    Shuffle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaylistSortMode {
    Name,
    Added,
}

#[derive(Clone, Debug)]
pub(crate) struct PlaylistNavigationContext {
    pub active_playlist_id: Option<String>,
    pub playback_playlist_id: Option<String>,
    pub loop_mode: PlaylistLoopMode,
    pub sort_mode: PlaylistSortMode,
    pub is_loop_one: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaylistNavigationResult {
    pub playlist_id: String,
    pub path: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedPlaylistEntry {
    pub path: String,
    pub title: Option<String>,
    pub artwork_ref: Option<String>,
    pub added_at: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedPlaylist {
    pub name: String,
    pub entries: Vec<PreparedPlaylistEntry>,
}

/// SQLite-backed playlist domain gateway. It intentionally keeps no entry collection in memory:
/// callers read summaries or a bounded entry page, and every structural write is serialized.
pub(crate) struct PlaylistService {
    mutation_lock: Mutex<()>,
}

impl PlaylistService {
    pub(crate) fn new() -> Self {
        Self {
            mutation_lock: Mutex::new(()),
        }
    }

    pub(crate) fn list_summaries(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Vec<PlaylistSummary>, String> {
        let conn = media_db::open_db(app)?;
        list_summaries_from_connection(&conn)
    }

    pub(crate) fn list_entries(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<PlaylistEntryPage, String> {
        let conn = media_db::open_db(app)?;
        list_entries_from_connection(&conn, playlist_id, offset, limit)
    }

    pub(crate) fn get_summary(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
    ) -> Result<Option<PlaylistSummary>, String> {
        let conn = media_db::open_db(app)?;
        playlist_summary_from_connection(&conn, playlist_id)
    }

    pub(crate) fn snapshot(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<PlaylistSnapshotDto, String> {
        let conn = media_db::open_db(app)?;
        let (collection_revision, playback_playlist_id, loop_mode, sort_mode, is_loop_one):
            (i64, Option<String>, String, String, i64) = conn
            .query_row(
                "SELECT collection_revision, playback_playlist_id, loop_mode, sort_mode, is_loop_one
                 FROM playlist_state WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .map_err(|error| error.to_string())?;
        Ok(PlaylistSnapshotDto {
            collection_revision: u64::try_from(collection_revision)
                .map_err(|_| "invalid negative collection revision".to_string())?,
            playlists: list_summaries_from_connection(&conn)?
                .iter()
                .map(playlist_summary_to_dto)
                .collect(),
            playback_playlist_id,
            loop_mode: if loop_mode == "shuffle" {
                PlaylistLoopModeDto::Shuffle
            } else {
                PlaylistLoopModeDto::List
            },
            sort_mode: if sort_mode == "added" {
                PlaylistSortModeDto::Added
            } else {
                PlaylistSortModeDto::Name
            },
            is_loop_one: is_loop_one != 0,
        })
    }

    pub(crate) fn get_entry(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        entry_id: &str,
    ) -> Result<Option<PlaylistEntry>, String> {
        let conn = media_db::open_db(app)?;
        conn.query_row(
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries WHERE playlist_id = ?1 AND id = ?2",
            params![playlist_id.trim(), entry_id.trim()],
            playlist_entry_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    pub(crate) fn resolve_navigation(
        &self,
        app: &tauri::AppHandle,
        current_path: &str,
        direction: i32,
        context: &PlaylistNavigationContext,
        is_end_of_file: bool,
    ) -> Result<Option<PlaylistNavigationResult>, String> {
        let conn = media_db::open_db(app)?;
        resolve_navigation_from_connection(&conn, current_path, direction, context, is_end_of_file)
    }

    pub(crate) fn title_for_path(
        &self,
        app: &tauri::AppHandle,
        path: &str,
        context: &PlaylistNavigationContext,
    ) -> Result<Option<String>, String> {
        let conn = media_db::open_db(app)?;
        title_for_path_from_connection(&conn, path, context)
    }

    pub(crate) fn create_playlist(
        &self,
        app: &tauri::AppHandle,
        name: &str,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let summary = create_playlist_in_transaction(&tx, name)?;
        bump_collection_revision(&tx)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn create_playlist_checked(
        &self,
        app: &tauri::AppHandle,
        name: &str,
        expected_collection_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        require_collection_revision(&tx, expected_collection_revision)?;
        let summary = create_playlist_in_transaction(&tx, name)?;
        bump_collection_revision(&tx)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    /// Persists one already-prepared playlist as a single transaction. Source recognition and
    /// parsing belong to the caller; this method only normalizes and stores domain data.
    pub(crate) fn import_prepared_playlist(
        &self,
        app: &tauri::AppHandle,
        prepared: PreparedPlaylist,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let summary = create_playlist_in_transaction(&tx, &prepared.name)?;
        insert_prepared_entries(&tx, &summary.id, &prepared.entries)?;
        bump_playlist_revision(&tx, &summary.id)?;
        bump_collection_revision(&tx)?;
        let summary = playlist_summary_in_transaction(&tx, &summary.id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn delete_playlist(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        expected_revision: i64,
    ) -> Result<(), String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        delete_playlist_in_transaction(&tx, playlist_id, expected_revision)?;
        tx.commit().map_err(|error| error.to_string())
    }

    pub(crate) fn delete_playlist_checked(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        expected_playlist_revision: i64,
        expected_collection_revision: i64,
    ) -> Result<(), String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        require_collection_revision(&tx, expected_collection_revision)?;
        delete_playlist_in_transaction(&tx, playlist_id, expected_playlist_revision)?;
        tx.commit().map_err(|error| error.to_string())
    }

    pub(crate) fn remove_entries(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        entry_ids: &[String],
        expected_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let playlist_id = require_playlist_revision(&tx, playlist_id, expected_revision)?;
        let entry_ids = entry_ids
            .iter()
            .map(|entry_id| normalized_id(entry_id))
            .collect::<Result<Vec<_>, _>>()?;
        if entry_ids.is_empty() || entry_ids.iter().collect::<HashSet<_>>().len() != entry_ids.len() {
            return Err("remove entries requires unique entry ids".to_string());
        }
        for entry_id in &entry_ids {
            let deleted = tx.execute(
                "DELETE FROM playlist_entries WHERE playlist_id = ?1 AND id = ?2",
                params![&playlist_id, entry_id],
            ).map_err(|error| error.to_string())?;
            if deleted == 0 {
                return Err("playlist entry not found".to_string());
            }
        }
        compact_entry_order(&tx, &playlist_id)?;
        finish_playlist_mutation(&tx, &playlist_id)?;
        let summary = playlist_summary_in_transaction(&tx, &playlist_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn reorder_playlists(
        &self,
        app: &tauri::AppHandle,
        playlist_ids: &[String],
        expected_collection_revision: i64,
    ) -> Result<(), String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        require_collection_revision(&tx, expected_collection_revision)?;
        let current_ids = playlist_ids_in_order(&tx)?;
        if playlist_ids.len() != current_ids.len()
            || playlist_ids.iter().collect::<HashSet<_>>().len() != playlist_ids.len()
            || playlist_ids.iter().collect::<HashSet<_>>() != current_ids.iter().collect::<HashSet<_>>() {
            return Err("playlist reorder must contain every playlist exactly once".to_string());
        }
        for (index, playlist_id) in playlist_ids.iter().enumerate() {
            tx.execute("UPDATE playlists SET order_index = ?2 WHERE id = ?1", params![playlist_id, -(index as i64) - 1])
                .map_err(|error| error.to_string())?;
        }
        for (index, playlist_id) in playlist_ids.iter().enumerate() {
            tx.execute("UPDATE playlists SET order_index = ?2, updated_at = ?3 WHERE id = ?1", params![playlist_id, index as i64, media_db::now_millis()])
                .map_err(|error| error.to_string())?;
        }
        bump_collection_revision(&tx)?;
        tx.commit().map_err(|error| error.to_string())
    }

    pub(crate) fn set_playback_preferences(
        &self,
        app: &tauri::AppHandle,
        playback_playlist_id: Option<Option<&str>>,
        loop_mode: Option<PlaylistLoopModeDto>,
        sort_mode: Option<PlaylistSortModeDto>,
        is_loop_one: Option<bool>,
    ) -> Result<(), String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        if let Some(playback_playlist_id) = playback_playlist_id {
            if let Some(playlist_id) = playback_playlist_id {
                let playlist_id = normalized_id(playlist_id)?;
                let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)", [&playlist_id], |row| row.get(0))
                    .map_err(|error| error.to_string())?;
                if !exists { return Err("playlist not found".to_string()); }
                tx.execute("UPDATE playlist_state SET playback_playlist_id = ?1, updated_at = ?2 WHERE singleton = 1", params![playlist_id, media_db::now_millis()])
                    .map_err(|error| error.to_string())?;
            } else {
                tx.execute("UPDATE playlist_state SET playback_playlist_id = NULL, updated_at = ?1 WHERE singleton = 1", [media_db::now_millis()])
                    .map_err(|error| error.to_string())?;
            }
        }
        if let Some(loop_mode) = loop_mode {
            let value = if matches!(loop_mode, PlaylistLoopModeDto::Shuffle) { "shuffle" } else { "list" };
            tx.execute("UPDATE playlist_state SET loop_mode = ?1, updated_at = ?2 WHERE singleton = 1", params![value, media_db::now_millis()])
                .map_err(|error| error.to_string())?;
        }
        if let Some(sort_mode) = sort_mode {
            let value = if matches!(sort_mode, PlaylistSortModeDto::Added) { "added" } else { "name" };
            tx.execute("UPDATE playlist_state SET sort_mode = ?1, updated_at = ?2 WHERE singleton = 1", params![value, media_db::now_millis()])
                .map_err(|error| error.to_string())?;
        }
        if let Some(is_loop_one) = is_loop_one {
            tx.execute("UPDATE playlist_state SET is_loop_one = ?1, updated_at = ?2 WHERE singleton = 1", params![i64::from(is_loop_one), media_db::now_millis()])
                .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())
    }

    pub(crate) fn rename_playlist(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        name: &str,
        expected_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        rename_playlist_in_transaction(&tx, playlist_id, name, expected_revision)?;
        let summary = playlist_summary_in_transaction(&tx, playlist_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn add_entries(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        entries: &[PreparedPlaylistEntry],
        expected_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        add_entries_in_transaction(&tx, playlist_id, entries, expected_revision)?;
        let summary = playlist_summary_in_transaction(&tx, playlist_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn remove_entry(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        entry_id: &str,
        expected_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        remove_entry_in_transaction(&tx, playlist_id, entry_id, expected_revision)?;
        let summary = playlist_summary_in_transaction(&tx, playlist_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn clear_entries(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        expected_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        clear_entries_in_transaction(&tx, playlist_id, expected_revision)?;
        let summary = playlist_summary_in_transaction(&tx, playlist_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn reorder_entries(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        entry_ids: &[String],
        expected_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        reorder_entries_in_transaction(&tx, playlist_id, entry_ids, expected_revision)?;
        let summary = playlist_summary_in_transaction(&tx, playlist_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }

    pub(crate) fn move_entry(
        &self,
        app: &tauri::AppHandle,
        playlist_id: &str,
        entry_id: &str,
        target_index: u32,
        expected_revision: i64,
    ) -> Result<PlaylistSummary, String> {
        let _guard = self.mutation_lock.lock().map_err(|error| error.to_string())?;
        let mut conn = media_db::open_db(app)?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        move_entry_in_transaction(&tx, playlist_id, entry_id, target_index, expected_revision)?;
        let summary = playlist_summary_in_transaction(&tx, playlist_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(summary)
    }
}

fn list_summaries_from_connection(conn: &Connection) -> Result<Vec<PlaylistSummary>, String> {
    let mut statement = conn
        .prepare(
            "SELECT p.id, p.name, COUNT(e.id), p.created_at, p.updated_at, p.order_index,
                    p.revision, p.is_protected
             FROM playlists p
             LEFT JOIN playlist_entries e ON e.playlist_id = p.id
             GROUP BY p.id
             ORDER BY p.order_index ASC, p.created_at ASC",
        )
        .map_err(|error| error.to_string())?;
    let summaries = statement
        .query_map([], playlist_summary_from_row)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(summaries)
}

fn playlist_summary_from_connection(
    conn: &Connection,
    playlist_id: &str,
) -> Result<Option<PlaylistSummary>, String> {
    let playlist_id = normalized_id(playlist_id)?;
    conn.query_row(
        "SELECT p.id, p.name, COUNT(e.id), p.created_at, p.updated_at, p.order_index,
                p.revision, p.is_protected
         FROM playlists p
         LEFT JOIN playlist_entries e ON e.playlist_id = p.id
         WHERE p.id = ?1
         GROUP BY p.id",
        [&playlist_id],
        playlist_summary_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn playlist_summary_to_dto(summary: &PlaylistSummary) -> PlaylistSummaryDto {
    PlaylistSummaryDto {
        id: summary.id.clone(),
        name: summary.name.clone(),
        created_at: summary.created_at,
        order_index: summary.order_index,
        revision: summary.revision.max(0) as u64,
        entry_count: summary.entry_count,
        is_protected: summary.is_protected,
    }
}

fn list_entries_from_connection(
    conn: &Connection,
    playlist_id: &str,
    offset: u32,
    limit: u32,
) -> Result<PlaylistEntryPage, String> {
    let playlist_id = normalized_id(playlist_id)?;
    let limit = i64::from(limit.clamp(1, MAX_ENTRY_PAGE_SIZE));
    let offset = i64::from(offset);
    let total = conn
        .query_row(
            "SELECT COUNT(*) FROM playlist_entries WHERE playlist_id = ?1",
            [&playlist_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut statement = conn
        .prepare(
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries
             WHERE playlist_id = ?1
             ORDER BY order_index ASC, added_at ASC
             LIMIT ?2 OFFSET ?3",
        )
        .map_err(|error| error.to_string())?;
    let entries = statement
        .query_map(params![playlist_id, limit, offset], |row| {
            Ok(PlaylistEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
                artwork_ref: row.get(3)?,
                added_at: row.get(4)?,
                order_index: row.get(5)?,
                revision: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(PlaylistEntryPage { entries, total, offset: offset as u32 })
}

fn resolve_navigation_from_connection(
    conn: &Connection,
    current_path: &str,
    direction: i32,
    context: &PlaylistNavigationContext,
    is_end_of_file: bool,
) -> Result<Option<PlaylistNavigationResult>, String> {
    let current_path = current_path.trim();
    if current_path.is_empty() || (is_end_of_file && context.is_loop_one) {
        return Ok(None);
    }
    let Some(playlist_id) = resolve_playlist_id_from_connection(conn, current_path, context)? else {
        return Ok(None);
    };
    let entry = match (context.loop_mode, context.sort_mode) {
        (PlaylistLoopMode::Shuffle, _) => {
            random_entry_from_connection(conn, &playlist_id, current_path)?
        }
        (PlaylistLoopMode::List, PlaylistSortMode::Added) => {
            adjacent_added_entry_from_connection(conn, &playlist_id, current_path, direction)?
        }
        // Name mode preserves the existing natural-sort behavior. Its durable sort-key migration
        // is intentionally deferred until measurement justifies the additional schema surface.
        (PlaylistLoopMode::List, PlaylistSortMode::Name) => {
            adjacent_name_entry_from_connection(conn, &playlist_id, current_path, direction)?
        }
    };
    let Some(entry) = entry else {
        return Ok(None);
    };
    Ok(Some(PlaylistNavigationResult {
        playlist_id,
        path: entry.path,
        title: entry.title,
    }))
}

fn adjacent_added_entry_from_connection(
    conn: &Connection,
    playlist_id: &str,
    current_path: &str,
    direction: i32,
) -> Result<Option<PlaylistEntry>, String> {
    let current: Option<(i64, i64)> = conn
        .query_row(
            "SELECT added_at, order_index FROM playlist_entries
             WHERE playlist_id = ?1 AND path = ?2",
            params![playlist_id, current_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let sql = match (current, direction > 0) {
        (Some((added_at, order_index)), true) => (
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries
             WHERE playlist_id = ?1
               AND (added_at < ?2 OR (added_at = ?2 AND order_index > ?3))
             ORDER BY added_at DESC, order_index ASC LIMIT 1",
            Some((added_at, order_index)),
        ),
        (Some((added_at, order_index)), false) => (
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries
             WHERE playlist_id = ?1
               AND (added_at > ?2 OR (added_at = ?2 AND order_index < ?3))
             ORDER BY added_at ASC, order_index DESC LIMIT 1",
            Some((added_at, order_index)),
        ),
        (None, true) => (
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries WHERE playlist_id = ?1
             ORDER BY added_at DESC, order_index ASC LIMIT 1",
            None,
        ),
        (None, false) => (
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries WHERE playlist_id = ?1
             ORDER BY added_at ASC, order_index DESC LIMIT 1",
            None,
        ),
    };
    let entry = match sql.1 {
        Some((added_at, order_index)) => conn
            .query_row(sql.0, params![playlist_id, added_at, order_index], playlist_entry_from_row)
            .optional(),
        None => conn.query_row(sql.0, [playlist_id], playlist_entry_from_row).optional(),
    }
    .map_err(|error| error.to_string())?;
    if entry.is_some() {
        return Ok(entry);
    }
    let wrap_sql = if direction > 0 {
        "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
         FROM playlist_entries WHERE playlist_id = ?1
         ORDER BY added_at DESC, order_index ASC LIMIT 1"
    } else {
        "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
         FROM playlist_entries WHERE playlist_id = ?1
         ORDER BY added_at ASC, order_index DESC LIMIT 1"
    };
    conn.query_row(wrap_sql, [playlist_id], playlist_entry_from_row)
        .optional()
        .map_err(|error| error.to_string())
}

fn random_entry_from_connection(
    conn: &Connection,
    playlist_id: &str,
    current_path: &str,
) -> Result<Option<PlaylistEntry>, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM playlist_entries WHERE playlist_id = ?1",
            [playlist_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if count == 0 {
        return Ok(None);
    }
    // Entry order is kept compact by every mutation. Choose an order index in Rust rather than
    // using SQLite's `ORDER BY RANDOM()`, which materializes and sorts the whole playlist.
    let current_order_index: Option<i64> = conn
        .query_row(
            "SELECT order_index FROM playlist_entries WHERE playlist_id = ?1 AND path = ?2",
            params![playlist_id, current_path],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let mut order_index = (uuid::Uuid::now_v7().as_u128() % count as u128) as i64;
    if count > 1 && current_order_index == Some(order_index) {
        order_index = (order_index + 1) % count;
    }
    let entry = conn
        .query_row(
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries WHERE playlist_id = ?1 AND order_index = ?2",
            params![playlist_id, order_index],
            playlist_entry_from_row,
        )
        .optional()
        .map_err(|error| error.to_string())?;

    // Existing installs may contain a partially migrated non-compact order. Recover without
    // returning the current item or reintroducing a random full-table sort.
    if entry.is_some() {
        return Ok(entry);
    }
    conn.query_row(
        "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
         FROM playlist_entries
         WHERE playlist_id = ?1 AND path != ?2
         ORDER BY order_index ASC LIMIT 1",
        params![playlist_id, current_path],
        playlist_entry_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn adjacent_name_entry_from_connection(
    conn: &Connection,
    playlist_id: &str,
    current_path: &str,
    direction: i32,
) -> Result<Option<PlaylistEntry>, String> {
    let mut entries = entries_for_navigation(conn, playlist_id)?;
    sort_entries_for_navigation(&mut entries, PlaylistSortMode::Name);
    if entries.is_empty() {
        return Ok(None);
    }
    let entry = match entries.iter().position(|entry| entry.path == current_path) {
        Some(index) if direction > 0 => entries[(index + 1) % entries.len()].clone(),
        Some(index) => entries[(index + entries.len() - 1) % entries.len()].clone(),
        None if direction > 0 => entries[0].clone(),
        None => entries.last().expect("entries is non-empty").clone(),
    };
    Ok(Some(entry))
}

fn title_for_path_from_connection(
    conn: &Connection,
    path: &str,
    context: &PlaylistNavigationContext,
) -> Result<Option<String>, String> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }
    let Some(playlist_id) = resolve_playlist_id_from_connection(conn, path, context)? else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT title FROM playlist_entries WHERE playlist_id = ?1 AND path = ?2",
        params![playlist_id, path],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|title| title.flatten().and_then(|title| non_empty(Some(&title))))
    .map_err(|error| error.to_string())
}

fn resolve_playlist_id_from_connection(
    conn: &Connection,
    path: &str,
    context: &PlaylistNavigationContext,
) -> Result<Option<String>, String> {
    for playlist_id in [
        context.playback_playlist_id.as_deref(),
        context.active_playlist_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM playlist_entries WHERE playlist_id = ?1 AND path = ?2)",
                params![playlist_id, path],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if exists {
            return Ok(Some(playlist_id.to_string()));
        }
    }
    conn.query_row(
        "SELECT p.id
         FROM playlists p
         JOIN playlist_entries e ON e.playlist_id = p.id
         WHERE e.path = ?1
         ORDER BY p.order_index DESC, p.created_at DESC
         LIMIT 1",
        [path],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn entries_for_navigation(conn: &Connection, playlist_id: &str) -> Result<Vec<PlaylistEntry>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, path, title, artwork_ref, added_at, order_index, record_version
             FROM playlist_entries WHERE playlist_id = ?1",
        )
        .map_err(|error| error.to_string())?;
    let entries = statement
        .query_map([playlist_id], |row| {
            Ok(PlaylistEntry {
                id: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
                artwork_ref: row.get(3)?,
                added_at: row.get(4)?,
                order_index: row.get(5)?,
                revision: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(entries)
}

fn playlist_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaylistEntry> {
    Ok(PlaylistEntry {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        artwork_ref: row.get(3)?,
        added_at: row.get(4)?,
        order_index: row.get(5)?,
        revision: row.get(6)?,
    })
}

fn sort_entries_for_navigation(entries: &mut [PlaylistEntry], mode: PlaylistSortMode) {
    match mode {
        PlaylistSortMode::Name => entries.sort_by(|left, right| {
            let left_name = left.title.as_deref().unwrap_or_else(|| path_display_name(&left.path));
            let right_name = right.title.as_deref().unwrap_or_else(|| path_display_name(&right.path));
            natural_sort_key(left_name).cmp(&natural_sort_key(right_name))
        }),
        PlaylistSortMode::Added => entries.sort_by(|left, right| right.added_at.cmp(&left.added_at)),
    }
}

fn path_display_name(path: &str) -> &str {
    path.rsplit(&['/', '\\'][..])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

fn natural_sort_key(input: &str) -> Vec<NaturalSegment> {
    let lower = input.to_lowercase();
    let mut chars = lower.chars().peekable();
    let mut segments = Vec::new();
    while chars.peek().is_some() {
        if chars.peek().is_some_and(|character| character.is_ascii_digit()) {
            let mut number = String::new();
            while chars.peek().is_some_and(|character| character.is_ascii_digit()) {
                number.push(chars.next().expect("peeked character exists"));
            }
            segments.push(NaturalSegment::Number(number.parse().unwrap_or(0)));
        } else {
            let mut text = String::new();
            while chars.peek().is_some_and(|character| !character.is_ascii_digit()) {
                text.push(chars.next().expect("peeked character exists"));
            }
            segments.push(NaturalSegment::Text(text));
        }
    }
    segments
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NaturalSegment {
    Text(String),
    Number(u64),
}

fn create_playlist_in_transaction(
    tx: &Transaction<'_>,
    name: &str,
) -> Result<PlaylistSummary, String> {
    let name = normalized_name(name)?;
    let order_index: i64 = tx
        .query_row("SELECT COALESCE(MAX(order_index) + 1, 0) FROM playlists", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    let id = media_db::new_uuid();
    let now = media_db::now_millis();
    tx.execute(
        "INSERT INTO playlists (id, name, created_at, updated_at, order_index, revision, is_protected)
         VALUES (?1, ?2, ?3, ?3, ?4, 1, 0)",
        params![id, name, now, order_index],
    )
    .map_err(|error| error.to_string())?;
    playlist_summary_in_transaction(tx, &id)
}

fn insert_prepared_entries(
    tx: &Transaction<'_>,
    playlist_id: &str,
    entries: &[PreparedPlaylistEntry],
) -> Result<(), String> {
    let mut seen_paths = HashSet::new();
    let now = media_db::now_millis();
    let mut order_index = 0_i64;
    for entry in entries {
        let path = entry.path.trim();
        if path.is_empty() || !seen_paths.insert(path.to_string()) {
            continue;
        }
        tx.execute(
            "INSERT INTO playlist_entries (
                 id, playlist_id, path, title, artwork_ref, order_index, added_at,
                 created_at, updated_at, record_version, last_modified_by_device_id, sync_status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1, 'playlist-service', 0)",
            params![
                media_db::new_uuid(),
                playlist_id,
                path,
                non_empty(entry.title.as_deref()),
                non_empty(entry.artwork_ref.as_deref()),
                order_index,
                entry.added_at,
                now,
            ],
        )
        .map_err(|error| error.to_string())?;
        order_index += 1;
    }
    Ok(())
}

fn delete_playlist_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
    expected_revision: i64,
) -> Result<(), String> {
    let playlist_id = normalized_id(playlist_id)?;
    let playlist: Option<(i64, i64)> = tx
        .query_row(
            "SELECT revision, is_protected FROM playlists WHERE id = ?1",
            [&playlist_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((revision, is_protected)) = playlist else {
        return Err("playlist not found".to_string());
    };
    if is_protected != 0 || playlist_id == FAVORITES_PLAYLIST_ID {
        return Err("favorites playlist cannot be deleted".to_string());
    }
    if revision != expected_revision {
        return Err(format!("playlist revision conflict: expected {expected_revision}, found {revision}"));
    }
    tx.execute("DELETE FROM playlist_entries WHERE playlist_id = ?1", [&playlist_id])
        .map_err(|error| error.to_string())?;
    tx.execute("DELETE FROM playlists WHERE id = ?1", [&playlist_id])
        .map_err(|error| error.to_string())?;
    bump_collection_revision(tx)
}

fn rename_playlist_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
    name: &str,
    expected_revision: i64,
) -> Result<(), String> {
    let playlist_id = require_editable_playlist(tx, playlist_id, expected_revision)?;
    tx.execute(
        "UPDATE playlists SET name = ?2 WHERE id = ?1",
        params![playlist_id, normalized_name(name)?],
    )
    .map_err(|error| error.to_string())?;
    finish_playlist_mutation(tx, &playlist_id)
}

fn add_entries_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
    entries: &[PreparedPlaylistEntry],
    expected_revision: i64,
) -> Result<(), String> {
    let playlist_id = require_playlist_revision(tx, playlist_id, expected_revision)?;
    let mut seen_paths = HashSet::new();
    let mut order_index: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(order_index) + 1, 0) FROM playlist_entries WHERE playlist_id = ?1",
            [&playlist_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let now = media_db::now_millis();
    for entry in entries {
        let path = entry.path.trim();
        if path.is_empty() || !seen_paths.insert(path.to_string()) {
            continue;
        }
        let inserted = tx.execute(
            "INSERT INTO playlist_entries (
                 id, playlist_id, path, title, artwork_ref, order_index, added_at,
                 created_at, updated_at, record_version, last_modified_by_device_id, sync_status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1, 'playlist-service', 0)
             ON CONFLICT(playlist_id, path) DO NOTHING",
            params![
                media_db::new_uuid(), &playlist_id, path, non_empty(entry.title.as_deref()),
                non_empty(entry.artwork_ref.as_deref()), order_index, entry.added_at, now,
            ],
        )
        .map_err(|error| error.to_string())?;
        if inserted != 0 {
            order_index += 1;
        }
    }
    finish_playlist_mutation(tx, &playlist_id)
}

fn remove_entry_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
    entry_id: &str,
    expected_revision: i64,
) -> Result<(), String> {
    let playlist_id = require_playlist_revision(tx, playlist_id, expected_revision)?;
    let entry_id = normalized_id(entry_id)?;
    let deleted = tx
        .execute(
            "DELETE FROM playlist_entries WHERE playlist_id = ?1 AND id = ?2",
            params![&playlist_id, entry_id],
        )
        .map_err(|error| error.to_string())?;
    if deleted == 0 {
        return Err("playlist entry not found".to_string());
    }
    compact_entry_order(tx, &playlist_id)?;
    finish_playlist_mutation(tx, &playlist_id)
}

fn clear_entries_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
    expected_revision: i64,
) -> Result<(), String> {
    let playlist_id = require_playlist_revision(tx, playlist_id, expected_revision)?;
    tx.execute("DELETE FROM playlist_entries WHERE playlist_id = ?1", [&playlist_id])
        .map_err(|error| error.to_string())?;
    finish_playlist_mutation(tx, &playlist_id)
}

fn reorder_entries_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
    entry_ids: &[String],
    expected_revision: i64,
) -> Result<(), String> {
    let playlist_id = require_playlist_revision(tx, playlist_id, expected_revision)?;
    let current_ids = entry_ids_in_order(tx, &playlist_id)?;
    if entry_ids.len() != current_ids.len()
        || entry_ids.iter().collect::<HashSet<_>>().len() != entry_ids.len()
        || entry_ids.iter().collect::<HashSet<_>>() != current_ids.iter().collect::<HashSet<_>>()
    {
        return Err("entry reorder must contain every playlist entry exactly once".to_string());
    }
    apply_entry_order(tx, &playlist_id, entry_ids)?;
    finish_playlist_mutation(tx, &playlist_id)
}

fn move_entry_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
    entry_id: &str,
    target_index: u32,
    expected_revision: i64,
) -> Result<(), String> {
    let playlist_id = require_playlist_revision(tx, playlist_id, expected_revision)?;
    let entry_id = normalized_id(entry_id)?;
    let mut entry_ids = entry_ids_in_order(tx, &playlist_id)?;
    let Some(current_index) = entry_ids.iter().position(|id| id == &entry_id) else {
        return Err("playlist entry not found".to_string());
    };
    let entry_id = entry_ids.remove(current_index);
    let target_index = (target_index as usize).min(entry_ids.len());
    entry_ids.insert(target_index, entry_id);
    apply_entry_order(tx, &playlist_id, &entry_ids)?;
    finish_playlist_mutation(tx, &playlist_id)
}

fn apply_entry_order(
    tx: &Transaction<'_>,
    playlist_id: &str,
    entry_ids: &[String],
) -> Result<(), String> {
    // A temporary negative range avoids the UNIQUE(playlist_id, order_index) collision while
    // shifting a large list; this remains one database transaction.
    for (index, entry_id) in entry_ids.iter().enumerate() {
        tx.execute(
            "UPDATE playlist_entries SET order_index = ?3 WHERE playlist_id = ?1 AND id = ?2",
            params![&playlist_id, entry_id, -(index as i64) - 1],
        )
        .map_err(|error| error.to_string())?;
    }
    for (index, entry_id) in entry_ids.iter().enumerate() {
        tx.execute(
            "UPDATE playlist_entries SET order_index = ?3, updated_at = ?4,
                 record_version = record_version + 1, last_modified_by_device_id = 'playlist-service',
                 sync_status = 1 WHERE playlist_id = ?1 AND id = ?2",
            params![&playlist_id, entry_id, index as i64, media_db::now_millis()],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn require_editable_playlist(
    tx: &Transaction<'_>,
    playlist_id: &str,
    expected_revision: i64,
) -> Result<String, String> {
    let playlist_id = require_playlist_revision(tx, playlist_id, expected_revision)?;
    if playlist_id == FAVORITES_PLAYLIST_ID {
        return Err("favorites playlist cannot be renamed".to_string());
    }
    let is_protected: i64 = tx
        .query_row("SELECT is_protected FROM playlists WHERE id = ?1", [&playlist_id], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if is_protected != 0 {
        return Err("protected playlist cannot be renamed".to_string());
    }
    Ok(playlist_id)
}

fn require_playlist_revision(
    tx: &Transaction<'_>,
    playlist_id: &str,
    expected_revision: i64,
) -> Result<String, String> {
    let playlist_id = normalized_id(playlist_id)?;
    let revision: Option<i64> = tx
        .query_row("SELECT revision FROM playlists WHERE id = ?1", [&playlist_id], |row| row.get(0))
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(revision) = revision else {
        return Err("playlist not found".to_string());
    };
    if revision != expected_revision {
        return Err(format!("playlist revision conflict: expected {expected_revision}, found {revision}"));
    }
    Ok(playlist_id)
}

fn require_collection_revision(
    tx: &Transaction<'_>,
    expected_revision: i64,
) -> Result<(), String> {
    let revision: i64 = tx
        .query_row(
            "SELECT collection_revision FROM playlist_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if revision != expected_revision {
        return Err(format!(
            "collection revision conflict: expected {expected_revision}, found {revision}"
        ));
    }
    Ok(())
}

fn playlist_ids_in_order(tx: &Transaction<'_>) -> Result<Vec<String>, String> {
    let mut statement = tx
        .prepare("SELECT id FROM playlists ORDER BY order_index ASC, created_at ASC, id ASC")
        .map_err(|error| error.to_string())?;
    let playlist_ids = statement
        .query_map([], |row| row.get(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(playlist_ids)
}

fn finish_playlist_mutation(tx: &Transaction<'_>, playlist_id: &str) -> Result<(), String> {
    bump_playlist_revision(tx, playlist_id)?;
    bump_collection_revision(tx)
}

fn compact_entry_order(tx: &Transaction<'_>, playlist_id: &str) -> Result<(), String> {
    let entry_ids = entry_ids_in_order(tx, playlist_id)?;
    apply_entry_order(tx, playlist_id, &entry_ids)
}

fn entry_ids_in_order(tx: &Transaction<'_>, playlist_id: &str) -> Result<Vec<String>, String> {
    let mut statement = tx
        .prepare(
            "SELECT id FROM playlist_entries WHERE playlist_id = ?1
             ORDER BY order_index ASC, added_at ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let entry_ids = statement
        .query_map([playlist_id], |row| row.get(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(entry_ids)
}

fn playlist_summary_in_transaction(
    tx: &Transaction<'_>,
    playlist_id: &str,
) -> Result<PlaylistSummary, String> {
    tx.query_row(
        "SELECT p.id, p.name, COUNT(e.id), p.created_at, p.updated_at, p.order_index,
                p.revision, p.is_protected
         FROM playlists p
         LEFT JOIN playlist_entries e ON e.playlist_id = p.id
         WHERE p.id = ?1
         GROUP BY p.id",
        [playlist_id],
        playlist_summary_from_row,
    )
    .map_err(|error| error.to_string())
}

fn playlist_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaylistSummary> {
    Ok(PlaylistSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        entry_count: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        order_index: row.get(5)?,
        revision: row.get(6)?,
        is_protected: row.get::<_, i64>(7)? != 0,
    })
}

fn bump_playlist_revision(tx: &Transaction<'_>, playlist_id: &str) -> Result<(), String> {
    tx.execute(
        "UPDATE playlists SET revision = revision + 1, updated_at = ?2 WHERE id = ?1",
        params![playlist_id, media_db::now_millis()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn bump_collection_revision(tx: &Transaction<'_>) -> Result<(), String> {
    tx.execute(
        "UPDATE playlist_state
         SET collection_revision = collection_revision + 1, updated_at = ?1
         WHERE singleton = 1",
        [media_db::now_millis()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn normalized_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err("playlist name is required".to_string())
    } else {
        Ok(value.to_string())
    }
}

fn normalized_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err("playlist id is required".to_string())
    } else {
        Ok(value.to_string())
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE playlists (
                 id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL, order_index INTEGER NOT NULL,
                 revision INTEGER NOT NULL, is_protected INTEGER NOT NULL
             );
             CREATE TABLE playlist_state (
                 singleton INTEGER PRIMARY KEY, collection_revision INTEGER NOT NULL,
                 playback_playlist_id TEXT, loop_mode TEXT NOT NULL, sort_mode TEXT NOT NULL,
                 is_loop_one INTEGER NOT NULL, updated_at INTEGER NOT NULL
             );
             CREATE TABLE playlist_entries (
                 id TEXT PRIMARY KEY, playlist_id TEXT NOT NULL, path TEXT NOT NULL,
                 title TEXT, artwork_ref TEXT, order_index INTEGER NOT NULL,
                 added_at INTEGER NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                 record_version INTEGER NOT NULL, last_modified_by_device_id TEXT NOT NULL,
                 sync_status INTEGER NOT NULL, UNIQUE(playlist_id, path),
                 UNIQUE(playlist_id, order_index)
             );
             INSERT INTO playlist_state (
                 singleton, collection_revision, playback_playlist_id, loop_mode, sort_mode,
                 is_loop_one, updated_at
             ) VALUES (1, 1, NULL, 'list', 'added', 0, 0);",
        )
        .expect("create playlist tables");
        conn
    }

    #[test]
    fn prepared_import_deduplicates_entries_and_supports_bounded_reads() {
        let mut conn = connection();
        let tx = conn.transaction().expect("begin import");
        let summary = create_playlist_in_transaction(&tx, " IPTV ").expect("create playlist");
        insert_prepared_entries(
            &tx,
            &summary.id,
            &[
                PreparedPlaylistEntry {
                    path: " https://example.test/one ".to_string(),
                    title: Some(" One ".to_string()),
                    artwork_ref: None,
                    added_at: 1,
                },
                PreparedPlaylistEntry {
                    path: "https://example.test/one".to_string(),
                    title: Some("duplicate".to_string()),
                    artwork_ref: None,
                    added_at: 2,
                },
                PreparedPlaylistEntry {
                    path: "https://example.test/two".to_string(),
                    title: None,
                    artwork_ref: None,
                    added_at: 3,
                },
            ],
        )
        .expect("insert prepared entries");
        bump_playlist_revision(&tx, &summary.id).expect("bump revision");
        bump_collection_revision(&tx).expect("bump collection revision");
        tx.commit().expect("commit import");

        let page = list_entries_from_connection(&conn, &summary.id, 1, 1).expect("read page");
        assert_eq!(page.total, 2);
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].path, "https://example.test/two");
        let summaries = list_summaries_from_connection(&conn).expect("read summaries");
        assert_eq!(summaries[0].name, "IPTV");
        assert_eq!(summaries[0].entry_count, 2);
        assert_eq!(summaries[0].revision, 2);
    }

    #[test]
    fn delete_rejects_favorites_and_revision_conflicts() {
        let mut conn = connection();
        let tx = conn.transaction().expect("begin create");
        let summary = create_playlist_in_transaction(&tx, "Delete me").expect("create playlist");
        tx.commit().expect("commit create");

        let tx = conn.transaction().expect("begin conflict delete");
        let error = delete_playlist_in_transaction(&tx, &summary.id, 99).expect_err("revision conflict");
        assert!(error.contains("revision conflict"));
        tx.rollback().expect("rollback conflict delete");

        conn.execute(
            "INSERT INTO playlists (id, name, created_at, updated_at, order_index, revision, is_protected)
             VALUES ('favorites', 'Favorites', 1, 1, 10, 1, 1)",
            [],
        )
        .expect("insert favorites");
        let tx = conn.transaction().expect("begin favorites delete");
        let error = delete_playlist_in_transaction(&tx, FAVORITES_PLAYLIST_ID, 1)
            .expect_err("favorites are protected");
        assert!(error.contains("cannot be deleted"));
    }

    #[test]
    fn entry_mutations_are_revision_checked_and_keep_order_compact() {
        let mut conn = connection();
        let tx = conn.transaction().expect("begin create");
        let summary = create_playlist_in_transaction(&tx, "Editable").expect("create playlist");
        add_entries_in_transaction(
            &tx,
            &summary.id,
            &[
                PreparedPlaylistEntry { path: "one".to_string(), added_at: 1, ..Default::default() },
                PreparedPlaylistEntry { path: "two".to_string(), added_at: 2, ..Default::default() },
                PreparedPlaylistEntry { path: "three".to_string(), added_at: 3, ..Default::default() },
            ],
            1,
        )
        .expect("add entries");
        tx.commit().expect("commit entries");

        let entries = list_entries_from_connection(&conn, &summary.id, 0, 10).expect("read entries");
        let reordered = vec![
            entries.entries[2].id.clone(),
            entries.entries[0].id.clone(),
            entries.entries[1].id.clone(),
        ];
        let tx = conn.transaction().expect("begin reorder");
        reorder_entries_in_transaction(&tx, &summary.id, &reordered, 2).expect("reorder entries");
        tx.commit().expect("commit reorder");

        let entries = list_entries_from_connection(&conn, &summary.id, 0, 10).expect("read reordered entries");
        assert_eq!(
            entries.entries.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>(),
            vec!["three", "one", "two"]
        );
        let tx = conn.transaction().expect("begin remove");
        remove_entry_in_transaction(&tx, &summary.id, &reordered[1], 3).expect("remove entry");
        tx.commit().expect("commit remove");

        let entries = list_entries_from_connection(&conn, &summary.id, 0, 10).expect("read compacted entries");
        assert_eq!(entries.entries.len(), 2);
        assert_eq!(entries.entries[0].order_index, 0);
        assert_eq!(entries.entries[1].order_index, 1);
        let tx = conn.transaction().expect("begin move");
        move_entry_in_transaction(&tx, &summary.id, &entries.entries[1].id, 0, 4)
            .expect("move entry");
        tx.commit().expect("commit move");
        let entries = list_entries_from_connection(&conn, &summary.id, 0, 10).expect("read moved entries");
        assert_eq!(entries.entries[0].path, "two");
        let tx = conn.transaction().expect("begin stale rename");
        let error = rename_playlist_in_transaction(&tx, &summary.id, "Renamed", 4)
            .expect_err("stale revision should fail");
        assert!(error.contains("revision conflict"));
    }

    #[test]
    fn favorites_allow_entry_edits_but_not_rename() {
        let mut conn = connection();
        conn.execute(
            "INSERT INTO playlists (id, name, created_at, updated_at, order_index, revision, is_protected)
             VALUES ('favorites', 'Favorites', 1, 1, 0, 1, 1)",
            [],
        )
        .expect("insert favorites");
        let tx = conn.transaction().expect("begin entry add");
        add_entries_in_transaction(
            &tx,
            FAVORITES_PLAYLIST_ID,
            &[PreparedPlaylistEntry { path: "favorite-item".to_string(), added_at: 1, ..Default::default() }],
            1,
        )
        .expect("favorites allow entries");
        tx.commit().expect("commit entry add");

        let tx = conn.transaction().expect("begin rename");
        let error = rename_playlist_in_transaction(&tx, FAVORITES_PLAYLIST_ID, "Other", 2)
            .expect_err("favorites rename rejected");
        assert!(error.contains("cannot be renamed"));
    }

    #[test]
    fn navigation_uses_sqlite_entries_with_natural_sort_and_loop_one() {
        let mut conn = connection();
        let tx = conn.transaction().expect("begin create");
        let summary = create_playlist_in_transaction(&tx, "Navigation").expect("create playlist");
        add_entries_in_transaction(
            &tx,
            &summary.id,
            &[
                PreparedPlaylistEntry { path: "/track10.mp4".to_string(), added_at: 3, ..Default::default() },
                PreparedPlaylistEntry { path: "/track2.mp4".to_string(), added_at: 2, ..Default::default() },
                PreparedPlaylistEntry { path: "/track1.mp4".to_string(), added_at: 1, ..Default::default() },
            ],
            1,
        )
        .expect("add entries");
        tx.commit().expect("commit entries");

        let context = PlaylistNavigationContext {
            active_playlist_id: Some(summary.id.clone()),
            playback_playlist_id: None,
            loop_mode: PlaylistLoopMode::List,
            sort_mode: PlaylistSortMode::Name,
            is_loop_one: false,
        };
        let result = resolve_navigation_from_connection(
            &conn,
            "/track1.mp4",
            1,
            &context,
            false,
        )
        .expect("resolve navigation")
        .expect("next item");
        assert_eq!(result.path, "/track2.mp4");
        assert_eq!(result.playlist_id, summary.id);

        let loop_one = PlaylistNavigationContext { is_loop_one: true, ..context };
        assert!(resolve_navigation_from_connection(
            &conn,
            "/track1.mp4",
            1,
            &loop_one,
            true,
        )
        .expect("resolve eof navigation")
        .is_none());
    }

    #[test]
    fn added_navigation_reads_only_the_adjacent_entry_and_wraps() {
        let mut conn = connection();
        let tx = conn.transaction().expect("begin create");
        let summary = create_playlist_in_transaction(&tx, "Added navigation").expect("create playlist");
        add_entries_in_transaction(
            &tx,
            &summary.id,
            &[
                PreparedPlaylistEntry { path: "/newest.mp4".to_string(), added_at: 30, ..Default::default() },
                PreparedPlaylistEntry { path: "/middle.mp4".to_string(), added_at: 20, ..Default::default() },
                PreparedPlaylistEntry { path: "/oldest.mp4".to_string(), added_at: 10, ..Default::default() },
            ],
            1,
        )
        .expect("add entries");
        tx.commit().expect("commit entries");
        let context = PlaylistNavigationContext {
            active_playlist_id: Some(summary.id),
            playback_playlist_id: None,
            loop_mode: PlaylistLoopMode::List,
            sort_mode: PlaylistSortMode::Added,
            is_loop_one: false,
        };

        let next = resolve_navigation_from_connection(&conn, "/newest.mp4", 1, &context, false)
            .expect("resolve next")
            .expect("next entry");
        assert_eq!(next.path, "/middle.mp4");
        let wrapped = resolve_navigation_from_connection(&conn, "/oldest.mp4", 1, &context, false)
            .expect("resolve wrapped next")
            .expect("wrapped entry");
        assert_eq!(wrapped.path, "/newest.mp4");
    }

    #[test]
    fn shuffle_navigation_selects_an_entry_without_replaying_the_current_item() {
        let mut conn = connection();
        let tx = conn.transaction().expect("begin create");
        let summary = create_playlist_in_transaction(&tx, "Shuffle navigation").expect("create playlist");
        add_entries_in_transaction(
            &tx,
            &summary.id,
            &[
                PreparedPlaylistEntry { path: "/one.mp4".to_string(), added_at: 1, ..Default::default() },
                PreparedPlaylistEntry { path: "/two.mp4".to_string(), added_at: 2, ..Default::default() },
                PreparedPlaylistEntry { path: "/three.mp4".to_string(), added_at: 3, ..Default::default() },
            ],
            1,
        )
        .expect("add entries");
        tx.commit().expect("commit entries");
        let context = PlaylistNavigationContext {
            active_playlist_id: Some(summary.id),
            playback_playlist_id: None,
            loop_mode: PlaylistLoopMode::Shuffle,
            sort_mode: PlaylistSortMode::Added,
            is_loop_one: false,
        };

        for _ in 0..16 {
            let next = resolve_navigation_from_connection(&conn, "/one.mp4", 1, &context, false)
                .expect("resolve shuffle")
                .expect("shuffle entry");
            assert_ne!(next.path, "/one.mp4");
        }
    }
}
