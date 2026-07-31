use crate::store::media_db;
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaylistEntryPage {
    pub entries: Vec<PlaylistEntry>,
    pub total: i64,
    pub offset: u32,
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
            "SELECT id, path, title, artwork_ref, added_at, order_index
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
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(PlaylistEntryPage { entries, total, offset: offset as u32 })
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
}
