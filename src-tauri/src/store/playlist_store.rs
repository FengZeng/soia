use crate::store::media_db;
use rusqlite::{params, OptionalExtension, Transaction};

pub const FAVORITES_PLAYLIST_ID: &str = "favorites";
const LEGACY_FAVOURITE_PLAYLIST_ID: &str = "favourite";
const FAVORITES_PLAYLIST_NAME: &str = "Favorites";

#[derive(Clone, Debug, Default)]
pub struct PersistedPlaylistEntry {
    pub path: String,
    pub title: Option<String>,
    pub artwork_ref: Option<String>,
    pub added_at: i64,
}

#[derive(Clone, Debug, Default)]
pub struct PersistedPlaylist {
    pub id: String,
    pub name: String,
    pub entries: Vec<PersistedPlaylistEntry>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default)]
pub struct PlaylistPersistenceState {
    pub playlists: Vec<PersistedPlaylist>,
    pub playlist_loop_mode: Option<String>,
    pub playlist_sort_mode: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PlaylistNavigationPreferences {
    pub playlist_loop_mode: Option<String>,
    pub playlist_sort_mode: Option<String>,
}

pub fn migrate_legacy_state(
    app: &tauri::AppHandle,
    state: &PlaylistPersistenceState,
) -> Result<(), String> {
    let mut conn = media_db::open_db(app)?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    import_legacy_state(&tx, state)?;
    tx.commit().map_err(|error| error.to_string())
}

pub fn load_state(app: &tauri::AppHandle) -> Result<PlaylistPersistenceState, String> {
    let conn = media_db::open_db(app)?;
    load_state_from_connection(&conn)
}

pub fn load_navigation_preferences(
    app: &tauri::AppHandle,
) -> Result<PlaylistNavigationPreferences, String> {
    let conn = media_db::open_db(app)?;
    let (loop_mode, sort_mode) = conn
        .query_row(
            "SELECT loop_mode, sort_mode FROM playlist_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or(("list".to_string(), "added".to_string()));
    Ok(PlaylistNavigationPreferences {
        playlist_loop_mode: Some(loop_mode),
        playlist_sort_mode: Some(sort_mode),
    })
}

pub fn save_state(
    app: &tauri::AppHandle,
    state: &PlaylistPersistenceState,
) -> Result<(), String> {
    let mut conn = media_db::open_db(app)?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    replace_state(&tx, state)?;
    tx.commit().map_err(|error| error.to_string())
}

fn normalize_playlist_id(id: &str) -> String {
    match id.trim() {
        LEGACY_FAVOURITE_PLAYLIST_ID => FAVORITES_PLAYLIST_ID.to_string(),
        value if value.is_empty() => media_db::new_uuid(),
        value => value.to_string(),
    }
}

fn normalized_playlists(playlists: &[PersistedPlaylist]) -> Vec<PersistedPlaylist> {
    let mut result = Vec::new();
    for playlist in playlists {
        let id = normalize_playlist_id(&playlist.id);
        if result.iter().any(|item: &PersistedPlaylist| item.id == id) {
            continue;
        }
        let name = if id == FAVORITES_PLAYLIST_ID {
            FAVORITES_PLAYLIST_NAME.to_string()
        } else {
            let name = playlist.name.trim();
            if name.is_empty() {
                format!("Playlist {}", result.len() + 1)
            } else {
                name.to_string()
            }
        };
        let mut entries = Vec::new();
        for entry in &playlist.entries {
            let path = entry.path.trim();
            if path.is_empty()
                || entries
                    .iter()
                    .any(|item: &PersistedPlaylistEntry| item.path == path)
            {
                continue;
            }
            entries.push(PersistedPlaylistEntry {
                path: path.to_string(),
                title: non_empty(entry.title.as_deref()),
                artwork_ref: non_empty(entry.artwork_ref.as_deref()),
                added_at: entry.added_at,
            });
        }
        result.push(PersistedPlaylist {
            id,
            name,
            entries,
            created_at: playlist.created_at,
        });
    }
    result
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn import_legacy_state(
    tx: &Transaction<'_>,
    state: &PlaylistPersistenceState,
) -> Result<(), String> {
    for (order_index, playlist) in normalized_playlists(&state.playlists).iter().enumerate() {
        insert_playlist(tx, playlist, order_index as i64, true)?;
    }
    update_playlist_state(tx, state, false)
}

fn replace_state(tx: &Transaction<'_>, state: &PlaylistPersistenceState) -> Result<(), String> {
    let playback_playlist_id: Option<String> = tx
        .query_row(
            "SELECT playback_playlist_id FROM playlist_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .flatten();
    let mut playlists = normalized_playlists(&state.playlists);
    if !playlists.iter().any(|playlist| playlist.id == FAVORITES_PLAYLIST_ID) {
        playlists.insert(0, PersistedPlaylist {
            id: FAVORITES_PLAYLIST_ID.to_string(),
            name: FAVORITES_PLAYLIST_NAME.to_string(),
            entries: Vec::new(),
            created_at: 0,
        });
    }
    tx.execute("DELETE FROM playlist_entries", [])
        .map_err(|error| error.to_string())?;
    tx.execute("DELETE FROM playlists", [])
        .map_err(|error| error.to_string())?;
    for (order_index, playlist) in playlists.iter().enumerate() {
        insert_playlist(tx, playlist, order_index as i64, false)?;
    }
    if let Some(playback_playlist_id) = playback_playlist_id
        .filter(|id| playlists.iter().any(|playlist| playlist.id == *id))
    {
        tx.execute(
            "UPDATE playlist_state SET playback_playlist_id = ?1 WHERE singleton = 1",
            [&playback_playlist_id],
        )
        .map_err(|error| error.to_string())?;
    }
    update_playlist_state(tx, state, true)
}

fn insert_playlist(
    tx: &Transaction<'_>,
    playlist: &PersistedPlaylist,
    order_index: i64,
    preserve_existing: bool,
) -> Result<(), String> {
    let now = media_db::now_millis();
    let is_protected = i64::from(playlist.id == FAVORITES_PLAYLIST_ID);
    let conflict = if preserve_existing { "ON CONFLICT(id) DO NOTHING" } else { "" };
    tx.execute(
        &format!(
            "INSERT INTO playlists (id, name, created_at, updated_at, order_index, revision, is_protected)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6) {conflict}"
        ),
        params![
            &playlist.id,
            &playlist.name,
            playlist.created_at,
            now,
            order_index,
            is_protected,
        ],
    )
    .map_err(|error| error.to_string())?;
    for (entry_order, entry) in playlist.entries.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlist_entries (
                 id, playlist_id, path, title, artwork_ref, order_index, added_at,
                 created_at, updated_at, record_version, last_modified_by_device_id, sync_status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7, 1, 'playlist-compat', 0)
             ON CONFLICT(playlist_id, path) DO UPDATE SET
                 title = COALESCE(NULLIF(playlist_entries.title, ''), excluded.title),
                 artwork_ref = COALESCE(playlist_entries.artwork_ref, excluded.artwork_ref)",
            params![
                media_db::new_uuid(),
                &playlist.id,
                &entry.path,
                entry.title.as_deref(),
                entry.artwork_ref.as_deref(),
                entry_order as i64,
                entry.added_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn update_playlist_state(
    tx: &Transaction<'_>,
    state: &PlaylistPersistenceState,
    replace: bool,
) -> Result<(), String> {
    let loop_mode = normalize_loop_mode(state.playlist_loop_mode.as_deref());
    let sort_mode = normalize_sort_mode(state.playlist_sort_mode.as_deref());
    let now = media_db::now_millis();
    if replace {
        tx.execute(
            "INSERT INTO playlist_state (singleton, collection_revision, loop_mode, sort_mode, is_loop_one, updated_at)
             VALUES (1, 1, ?1, ?2, 0, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
                 loop_mode = excluded.loop_mode,
                 sort_mode = excluded.sort_mode,
                 collection_revision = playlist_state.collection_revision + 1,
                 updated_at = excluded.updated_at",
            params![loop_mode, sort_mode, now],
        )
    } else {
        tx.execute(
            "INSERT INTO playlist_state (singleton, collection_revision, loop_mode, sort_mode, is_loop_one, updated_at)
             VALUES (1, 1, ?1, ?2, 0, ?3)
             ON CONFLICT(singleton) DO NOTHING",
            params![loop_mode, sort_mode, now],
        )
    }
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn load_state_from_connection(conn: &rusqlite::Connection) -> Result<PlaylistPersistenceState, String> {
    let mut playlist_statement = conn
        .prepare("SELECT id, name, created_at FROM playlists ORDER BY order_index ASC, created_at ASC")
        .map_err(|error| error.to_string())?;
    let rows = playlist_statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))
        .map_err(|error| error.to_string())?;
    let mut playlists = Vec::new();
    for row in rows {
        let (id, name, created_at) = row.map_err(|error| error.to_string())?;
        let mut entries = conn
            .prepare("SELECT path, title, artwork_ref, added_at FROM playlist_entries WHERE playlist_id = ?1 ORDER BY order_index ASC, added_at ASC")
            .map_err(|error| error.to_string())?;
        let entry_rows = entries
            .query_map([&id], |row| Ok(PersistedPlaylistEntry {
                path: row.get(0)?,
                title: non_empty(row.get::<_, Option<String>>(1)?.as_deref()),
                artwork_ref: row.get(2)?,
                added_at: row.get(3)?,
            }))
            .map_err(|error| error.to_string())?;
        let entries = entry_rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
        playlists.push(PersistedPlaylist { id, name, entries, created_at });
    }
    let (loop_mode, sort_mode) = conn
        .query_row(
            "SELECT loop_mode, sort_mode FROM playlist_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .unwrap_or(("list".to_string(), "added".to_string()));
    Ok(PlaylistPersistenceState {
        playlists,
        playlist_loop_mode: Some(loop_mode),
        playlist_sort_mode: Some(sort_mode),
    })
}

fn normalize_loop_mode(value: Option<&str>) -> &str {
    if value == Some("shuffle") { "shuffle" } else { "list" }
}

fn normalize_sort_mode(value: Option<&str>) -> &str {
    if value == Some("name") { "name" } else { "added" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE playlists (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 order_index INTEGER NOT NULL,
                 revision INTEGER NOT NULL,
                 is_protected INTEGER NOT NULL
             );
             CREATE TABLE playlist_state (
                 singleton INTEGER PRIMARY KEY,
                 collection_revision INTEGER NOT NULL,
                 playback_playlist_id TEXT,
                 loop_mode TEXT NOT NULL,
                 sort_mode TEXT NOT NULL,
                 is_loop_one INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 FOREIGN KEY(playback_playlist_id) REFERENCES playlists(id) ON DELETE SET NULL
             );
             CREATE TABLE playlist_entries (
                 id TEXT PRIMARY KEY,
                 playlist_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 title TEXT,
                 artwork_ref TEXT,
                 order_index INTEGER NOT NULL,
                 added_at INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 record_version INTEGER NOT NULL,
                 last_modified_by_device_id TEXT NOT NULL,
                 sync_status INTEGER NOT NULL,
                 UNIQUE(playlist_id, path),
                 UNIQUE(playlist_id, order_index),
                 FOREIGN KEY(playlist_id) REFERENCES playlists(id) ON DELETE CASCADE
             );",
        )
        .expect("create playlist schema");
        conn
    }

    fn sample_state() -> PlaylistPersistenceState {
        PlaylistPersistenceState {
            playlists: vec![PersistedPlaylist {
                id: "favourite".to_string(),
                name: "old name".to_string(),
                created_at: 10,
                entries: vec![PersistedPlaylistEntry {
                    path: " https://example.test/news.m3u8 ".to_string(),
                    title: Some(" News ".to_string()),
                    artwork_ref: Some(" https://example.test/news.jpg ".to_string()),
                    added_at: 11,
                }],
            }],
            playlist_loop_mode: Some("shuffle".to_string()),
            playlist_sort_mode: Some("name".to_string()),
        }
    }

    #[test]
    fn legacy_import_is_idempotent_and_normalizes_favorites() {
        let mut conn = connection();
        let state = sample_state();

        let tx = conn.transaction().expect("begin import");
        import_legacy_state(&tx, &state).expect("first import");
        tx.commit().expect("commit first import");
        let tx = conn.transaction().expect("begin repeated import");
        import_legacy_state(&tx, &state).expect("repeated import");
        tx.commit().expect("commit repeated import");

        let (id, name, is_protected): (String, String, i64) = conn
            .query_row(
                "SELECT id, name, is_protected FROM playlists",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read normalized playlist");
        assert_eq!(id, FAVORITES_PLAYLIST_ID);
        assert_eq!(name, FAVORITES_PLAYLIST_NAME);
        assert_eq!(is_protected, 1);

        let (count, path, title, artwork): (i64, String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT COUNT(*), MIN(path), MIN(title), MIN(artwork_ref) FROM playlist_entries",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("read imported entry");
        assert_eq!(count, 1);
        assert_eq!(path, "https://example.test/news.m3u8");
        assert_eq!(title.as_deref(), Some("News"));
        assert_eq!(artwork.as_deref(), Some("https://example.test/news.jpg"));
    }

    #[test]
    fn compatibility_save_and_load_round_trip_preserves_entries_and_modes() {
        let mut conn = connection();
        let state = sample_state();

        let tx = conn.transaction().expect("begin save");
        replace_state(&tx, &state).expect("save state");
        tx.commit().expect("commit save");

        let loaded = load_state_from_connection(&conn).expect("load state");
        assert_eq!(loaded.playlist_loop_mode.as_deref(), Some("shuffle"));
        assert_eq!(loaded.playlist_sort_mode.as_deref(), Some("name"));
        assert_eq!(loaded.playlists.len(), 1);
        let favorites = &loaded.playlists[0];
        assert_eq!(favorites.id, FAVORITES_PLAYLIST_ID);
        assert_eq!(favorites.entries.len(), 1);
        assert_eq!(favorites.entries[0].title.as_deref(), Some("News"));
        assert_eq!(
            favorites.entries[0].artwork_ref.as_deref(),
            Some("https://example.test/news.jpg")
        );
    }

    #[test]
    fn compatibility_save_keeps_an_existing_playback_playlist_reference() {
        let mut conn = connection();
        let state = PlaylistPersistenceState {
            playlists: vec![PersistedPlaylist {
                id: "watch-later".to_string(),
                name: "Watch later".to_string(),
                entries: Vec::new(),
                created_at: 10,
            }],
            ..PlaylistPersistenceState::default()
        };
        let tx = conn.transaction().expect("begin first save");
        replace_state(&tx, &state).expect("first save");
        tx.commit().expect("commit first save");
        conn.execute(
            "UPDATE playlist_state SET playback_playlist_id = 'watch-later' WHERE singleton = 1",
            [],
        )
        .expect("set playback playlist");

        let tx = conn.transaction().expect("begin repeated save");
        replace_state(&tx, &state).expect("repeated save");
        tx.commit().expect("commit repeated save");

        let playback_playlist_id: Option<String> = conn
            .query_row(
                "SELECT playback_playlist_id FROM playlist_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("read playback playlist");
        assert_eq!(playback_playlist_id.as_deref(), Some("watch-later"));
    }
}
