use crate::store::storage_paths;
use chrono::Utc;
use log::{info, warn};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub const MAX_PLAY_HISTORY: i64 = 100;
const SCHEMA_VERSION: i32 = 4;

pub fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn today_utc_date() -> String {
    Utc::now().date_naive().format("%Y-%m-%d").to_string()
}

pub fn new_uuid() -> String {
    Uuid::now_v7().to_string()
}

pub fn normalize_uuid_or_new(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return new_uuid();
    }
    Uuid::parse_str(trimmed)
        .map(|uuid| uuid.to_string())
        .unwrap_or_else(|_| new_uuid())
}

pub fn open_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = storage_paths::media_db_path(app)?;
    let mut conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;",
    )
    .map_err(|e| e.to_string())?;
    ensure_schema(&mut conn)?;
    Ok(conn)
}

pub fn local_device_id(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT device_id FROM app_installation_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

fn ensure_schema(conn: &mut Connection) -> Result<(), String> {
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let is_brand_new_db = version == 0 && !has_known_media_tables(conn)?;

    if version == 2 {
        migrate_schema_v2_to_v3(conn)?;
        migrate_schema_v3_to_v4(conn)?;
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
            .map_err(|e| e.to_string())?;
    } else if version == 3 {
        migrate_schema_v3_to_v4(conn)?;
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
            .map_err(|e| e.to_string())?;
    } else if version != SCHEMA_VERSION {
        warn!("media db schema version mismatch: found={version}, expected={SCHEMA_VERSION}");
        if !is_brand_new_db {
            return Err(format!(
                "Unsupported media.db schema version {version}; refusing to rebuild an existing database"
            ));
        }
        info!("media db: initializing new database");

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        reset_schema(&tx)?;
        tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }

    ensure_installation_state(conn, is_brand_new_db)?;
    ensure_local_device_record(conn)?;
    ensure_sync_state_rows(conn)?;
    ensure_playlist_state(conn)?;
    Ok(())
}

fn has_known_media_tables(conn: &Connection) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'table'
               AND name IN (
                   'app_installation_state',
                   'sync_devices',
                   'sync_accounts',
                   'playlists',
                   'playlist_state',
                   'playlist_entries',
                   'play_history',
                   'sync_state',
                   'sync_tombstones'
               )",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

fn reset_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS sync_tombstones;
         DROP TABLE IF EXISTS sync_state;
         DROP TABLE IF EXISTS playlist_state;
         DROP TABLE IF EXISTS playlists;
         DROP TABLE IF EXISTS playlist_entries;
         DROP TABLE IF EXISTS play_history;
         DROP TABLE IF EXISTS app_installation_state;
         DROP TABLE IF EXISTS sync_devices;
         DROP TABLE IF EXISTS sync_accounts;

         CREATE TABLE sync_accounts (
             id TEXT PRIMARY KEY,
             provider TEXT NOT NULL DEFAULT 'soia',
             remote_user_id TEXT UNIQUE,
             email TEXT,
             display_name TEXT,
             auth_data TEXT NOT NULL DEFAULT '{}',
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             last_login_at INTEGER,
             status INTEGER NOT NULL DEFAULT 0 CHECK (status IN (0, 1, 2))
         ) STRICT;

         CREATE TABLE sync_devices (
             id TEXT PRIMARY KEY,
             account_id TEXT,
             installation_id TEXT NOT NULL UNIQUE,
             device_name TEXT NOT NULL DEFAULT '',
             platform TEXT NOT NULL DEFAULT '',
             app_version TEXT NOT NULL DEFAULT '',
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             last_seen_at INTEGER,
             is_current INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
             FOREIGN KEY(account_id) REFERENCES sync_accounts(id) ON DELETE SET NULL
         ) STRICT;

         CREATE INDEX idx_sync_devices_account
         ON sync_devices(account_id);

         CREATE TABLE playlists (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             order_index INTEGER NOT NULL,
             revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
             is_protected INTEGER NOT NULL DEFAULT 0 CHECK (is_protected IN (0, 1))
         ) STRICT;

         CREATE INDEX idx_playlists_order
         ON playlists(order_index ASC, created_at ASC);

         CREATE TABLE playlist_state (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             collection_revision INTEGER NOT NULL DEFAULT 1 CHECK (collection_revision > 0),
             playback_playlist_id TEXT,
             loop_mode TEXT NOT NULL DEFAULT 'list' CHECK (loop_mode IN ('list', 'shuffle')),
             sort_mode TEXT NOT NULL DEFAULT 'added' CHECK (sort_mode IN ('name', 'added')),
             is_loop_one INTEGER NOT NULL DEFAULT 0 CHECK (is_loop_one IN (0, 1)),
             updated_at INTEGER NOT NULL,
             FOREIGN KEY(playback_playlist_id) REFERENCES playlists(id) ON DELETE SET NULL
         ) STRICT;

         CREATE TABLE playlist_entries (
             id TEXT PRIMARY KEY,
             playlist_id TEXT NOT NULL DEFAULT 'default',
             path TEXT NOT NULL,
             title TEXT,
             artwork_ref TEXT,
             order_index INTEGER NOT NULL,
             added_at INTEGER NOT NULL,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             record_version INTEGER NOT NULL DEFAULT 1 CHECK (record_version > 0),
             last_modified_by_device_id TEXT NOT NULL,
             sync_status INTEGER NOT NULL DEFAULT 0 CHECK (sync_status IN (0, 1, 2)),
             remote_record_id TEXT,
             remote_updated_at INTEGER,
             UNIQUE(playlist_id, path),
             UNIQUE(playlist_id, order_index)
         ) STRICT;

         CREATE INDEX idx_playlist_entries_order
         ON playlist_entries(playlist_id, order_index ASC);

         CREATE INDEX idx_playlist_entries_key
         ON playlist_entries(playlist_id, path);

         CREATE INDEX idx_playlist_entries_sync
         ON playlist_entries(sync_status, updated_at ASC);

         CREATE TABLE play_history (
             id TEXT PRIMARY KEY,
             path TEXT NOT NULL UNIQUE,
             title TEXT NOT NULL DEFAULT '',
             last_position REAL NOT NULL DEFAULT 0,
             duration REAL NOT NULL DEFAULT 0,
             last_played_at INTEGER NOT NULL,
             is_pinned INTEGER NOT NULL DEFAULT 0 CHECK (is_pinned IN (0, 1)),
             is_live_playback INTEGER NOT NULL DEFAULT 0 CHECK (is_live_playback IN (0, 1)),
             external_audio TEXT NOT NULL DEFAULT '[]',
             external_sub TEXT NOT NULL DEFAULT '[]',
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             record_version INTEGER NOT NULL DEFAULT 1 CHECK (record_version > 0),
             last_modified_by_device_id TEXT NOT NULL,
             sync_status INTEGER NOT NULL DEFAULT 0 CHECK (sync_status IN (0, 1, 2)),
             remote_record_id TEXT,
             remote_updated_at INTEGER
         ) STRICT;

         CREATE INDEX idx_play_history_sort
         ON play_history(is_pinned DESC, last_played_at DESC);

         CREATE INDEX idx_play_history_sync
         ON play_history(sync_status, updated_at ASC);

         CREATE TABLE sync_tombstones (
             id TEXT PRIMARY KEY,
             entity_type TEXT NOT NULL CHECK (entity_type IN ('playlist_entries', 'play_history')),
             entity_id TEXT NOT NULL,
             payload TEXT NOT NULL DEFAULT '{}',
             deleted_at INTEGER NOT NULL,
             record_version INTEGER NOT NULL DEFAULT 1 CHECK (record_version > 0),
             last_modified_by_device_id TEXT NOT NULL,
             sync_status INTEGER NOT NULL DEFAULT 2 CHECK (sync_status IN (0, 2)),
             UNIQUE(entity_type, entity_id)
         ) STRICT;

         CREATE INDEX idx_sync_tombstones_sync
         ON sync_tombstones(sync_status, deleted_at ASC);

         CREATE TABLE sync_state (
             scope TEXT PRIMARY KEY CHECK (scope IN ('playlist_entries', 'play_history', 'tombstones')),
             last_sync_token TEXT,
             last_synced_at INTEGER,
             last_full_sync_at INTEGER,
             last_error TEXT,
             updated_at INTEGER NOT NULL
         ) STRICT;

         CREATE TABLE app_installation_state (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             install_id TEXT NOT NULL UNIQUE,
             device_id TEXT NOT NULL UNIQUE,
             active_account_id TEXT,
             install_id_updated_at INTEGER NOT NULL,
             uuid_update_data TEXT NOT NULL DEFAULT '{}',
             last_dau_reported_date_utc TEXT,
             last_update_checked_date_utc TEXT,
             last_sync_started_at INTEGER,
             last_sync_finished_at INTEGER,
             updated_at INTEGER NOT NULL,
             FOREIGN KEY(active_account_id) REFERENCES sync_accounts(id) ON DELETE SET NULL
         ) STRICT;",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn migrate_schema_v2_to_v3(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "BEGIN;
         ALTER TABLE play_history
         ADD COLUMN is_live_playback INTEGER NOT NULL DEFAULT 0 CHECK (is_live_playback IN (0, 1));
         COMMIT;",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn migrate_schema_v3_to_v4(conn: &mut Connection) -> Result<(), String> {
    let now = now_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    if !table_has_column(&tx, "playlist_entries", "title")? {
        tx.execute("ALTER TABLE playlist_entries ADD COLUMN title TEXT", [])
            .map_err(|e| e.to_string())?;
    }
    if !table_has_column(&tx, "playlist_entries", "artwork_ref")? {
        tx.execute("ALTER TABLE playlist_entries ADD COLUMN artwork_ref TEXT", [])
            .map_err(|e| e.to_string())?;
    }
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS playlists (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             order_index INTEGER NOT NULL,
             revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
             is_protected INTEGER NOT NULL DEFAULT 0 CHECK (is_protected IN (0, 1))
         ) STRICT;

         CREATE INDEX IF NOT EXISTS idx_playlists_order
         ON playlists(order_index ASC, created_at ASC);

         CREATE TABLE IF NOT EXISTS playlist_state (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             collection_revision INTEGER NOT NULL DEFAULT 1 CHECK (collection_revision > 0),
             playback_playlist_id TEXT,
             loop_mode TEXT NOT NULL DEFAULT 'list' CHECK (loop_mode IN ('list', 'shuffle')),
             sort_mode TEXT NOT NULL DEFAULT 'added' CHECK (sort_mode IN ('name', 'added')),
             is_loop_one INTEGER NOT NULL DEFAULT 0 CHECK (is_loop_one IN (0, 1)),
             updated_at INTEGER NOT NULL,
             FOREIGN KEY(playback_playlist_id) REFERENCES playlists(id) ON DELETE SET NULL
         ) STRICT;

         CREATE INDEX IF NOT EXISTS idx_playlist_entries_key
         ON playlist_entries(playlist_id, path);",
    )
    .map_err(|e| e.to_string())?;
    if !table_has_column(&tx, "playlists", "updated_at")? {
        tx.execute(
            "ALTER TABLE playlists ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    if !table_has_column(&tx, "playlist_state", "updated_at")? {
        tx.execute(
            "ALTER TABLE playlist_state ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|e| e.to_string())?;
    }

    let has_default_entries: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM playlist_entries WHERE playlist_id = 'default')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if has_default_entries {
        let default_order_index: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(order_index), -1) + 1 FROM playlists",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO playlists (
                 id, name, created_at, updated_at, order_index, revision, is_protected
             ) VALUES ('default', 'Playlist', ?1, ?1, ?2, 1, 0)
             ON CONFLICT(id) DO NOTHING",
            params![now, default_order_index],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "INSERT INTO playlist_state (
             singleton, collection_revision, loop_mode, sort_mode, is_loop_one, updated_at
         ) VALUES (1, 1, 'list', 'added', 0, ?1)
         ON CONFLICT(singleton) DO NOTHING",
        [now],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let sql = format!("PRAGMA table_info({table})");
    let mut statement = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    for name in columns {
        if name.map_err(|e| e.to_string())? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_installation_state(conn: &Connection, allow_create: bool) -> Result<(), String> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM app_installation_state WHERE singleton = 1",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some();
    if exists {
        return Ok(());
    }
    if !allow_create {
        return Err(
            "Refusing to create new installation UUIDs for an existing media.db".to_string(),
        );
    }

    let now = now_millis();
    conn.execute(
        "INSERT INTO app_installation_state (
             singleton,
             install_id,
             device_id,
             install_id_updated_at,
             uuid_update_data,
             updated_at
         )
         VALUES (1, ?1, ?2, ?3, '{}', ?3)
         ON CONFLICT(singleton) DO NOTHING",
        params![new_uuid(), new_uuid(), now],
    )
    .map_err(|e| e.to_string())?;
    info!("media db: created initial installation UUIDs");
    Ok(())
}

fn ensure_local_device_record(conn: &Connection) -> Result<(), String> {
    let now = now_millis();
    let (install_id, device_id): (String, String) = conn
        .query_row(
            "SELECT install_id, device_id FROM app_installation_state WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE sync_devices SET is_current = 0 WHERE is_current = 1 AND id != ?1",
        params![&device_id],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO sync_devices (
             id,
             installation_id,
             device_name,
             platform,
             app_version,
             created_at,
             updated_at,
             last_seen_at,
             is_current
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6, 1)
         ON CONFLICT(id) DO UPDATE SET
             installation_id = excluded.installation_id,
             platform = excluded.platform,
             app_version = excluded.app_version,
             updated_at = excluded.updated_at,
             last_seen_at = excluded.last_seen_at,
             is_current = 1",
        params![
            &device_id,
            &install_id,
            local_device_name(),
            std::env::consts::OS,
            env!("CARGO_PKG_VERSION"),
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn ensure_sync_state_rows(conn: &Connection) -> Result<(), String> {
    let now = now_millis();
    for scope in ["playlist_entries", "play_history", "tombstones"] {
        conn.execute(
            "INSERT INTO sync_state (scope, updated_at)
             VALUES (?1, ?2)
             ON CONFLICT(scope) DO NOTHING",
            params![scope, now],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn ensure_playlist_state(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "INSERT INTO playlist_state (
             singleton, collection_revision, loop_mode, sort_mode, is_loop_one, updated_at
         ) VALUES (1, 1, 'list', 'added', 0, ?1)
         ON CONFLICT(singleton) DO NOTHING",
        [now_millis()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn local_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "local-device".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALL_ID: &str = "019d27fa-c0fa-7ef2-8bbb-a374c0dbb00c";
    const DEVICE_ID: &str = "019e91b4-f62f-7cb1-8328-f109e5434d27";

    fn memory_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )
        .expect("set pragmas");
        conn
    }

    #[test]
    fn brand_new_db_creates_installation_state() {
        let mut conn = memory_conn();

        ensure_schema(&mut conn).expect("ensure schema");

        let (install_id, device_id): (String, String) = conn
            .query_row(
                "SELECT install_id, device_id
                 FROM app_installation_state
                 WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read installation state");
        Uuid::parse_str(&install_id).expect("valid install id");
        Uuid::parse_str(&device_id).expect("valid device id");

        let state_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlist_state WHERE singleton = 1", [], |row| {
                row.get(0)
            })
            .expect("read playlist state");
        assert_eq!(state_count, 1);
    }

    #[test]
    fn schema_v3_migration_preserves_default_playlist_entries() {
        let mut conn = memory_conn();
        conn.execute_batch(
            "CREATE TABLE playlist_entries (
                 id TEXT PRIMARY KEY,
                 playlist_id TEXT NOT NULL DEFAULT 'default',
                 path TEXT NOT NULL,
                 order_index INTEGER NOT NULL,
                 added_at INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 record_version INTEGER NOT NULL DEFAULT 1,
                 last_modified_by_device_id TEXT NOT NULL,
                 sync_status INTEGER NOT NULL DEFAULT 0,
                 remote_record_id TEXT,
                 remote_updated_at INTEGER,
                 UNIQUE(playlist_id, path),
                 UNIQUE(playlist_id, order_index)
             ) STRICT;
             CREATE INDEX idx_playlist_entries_order
             ON playlist_entries(playlist_id, order_index ASC);",
        )
        .expect("create v3 playlist entries");
        conn.execute(
            "INSERT INTO playlist_entries (
                 id, playlist_id, path, order_index, added_at, created_at, updated_at,
                 record_version, last_modified_by_device_id, sync_status
             ) VALUES ('entry-1', 'default', '/media/one.mp4', 0, 1, 1, 1, 1, 'device-1', 0)",
            [],
        )
        .expect("insert v3 playlist entry");

        migrate_schema_v3_to_v4(&mut conn).expect("migrate v3 schema");
        migrate_schema_v3_to_v4(&mut conn).expect("resume partially applied v3 migration");

        let (path, title, artwork_ref): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT path, title, artwork_ref FROM playlist_entries WHERE id = 'entry-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read migrated entry");
        assert_eq!(path, "/media/one.mp4");
        assert_eq!(title, None);
        assert_eq!(artwork_ref, None);

        let default_playlist_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM playlists WHERE id = 'default'",
                [],
                |row| row.get(0),
            )
            .expect("read default playlist");
        assert_eq!(default_playlist_count, 1);

        let collection_revision: i64 = conn
            .query_row(
                "SELECT collection_revision FROM playlist_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("read playlist state");
        assert_eq!(collection_revision, 1);
    }

    #[test]
    fn schema_v3_migration_resumes_legacy_playlist_tables() {
        let mut conn = memory_conn();
        conn.execute_batch(
            "CREATE TABLE playlist_entries (
                 id TEXT PRIMARY KEY,
                 playlist_id TEXT NOT NULL DEFAULT 'default',
                 path TEXT NOT NULL,
                 order_index INTEGER NOT NULL,
                 added_at INTEGER NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 record_version INTEGER NOT NULL DEFAULT 1,
                 last_modified_by_device_id TEXT NOT NULL,
                 sync_status INTEGER NOT NULL DEFAULT 0,
                 remote_record_id TEXT,
                 remote_updated_at INTEGER,
                 UNIQUE(playlist_id, path),
                 UNIQUE(playlist_id, order_index)
             ) STRICT;
             CREATE TABLE playlists (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 order_index INTEGER NOT NULL,
                 revision INTEGER NOT NULL DEFAULT 1,
                 is_protected INTEGER NOT NULL DEFAULT 0,
                 UNIQUE(order_index)
             ) STRICT;
             CREATE TABLE playlist_state (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 active_playlist_id TEXT,
                 playback_playlist_id TEXT,
                 loop_mode TEXT NOT NULL DEFAULT 'list',
                 sort_mode TEXT NOT NULL DEFAULT 'added',
                 is_loop_one INTEGER NOT NULL DEFAULT 0,
                 collection_revision INTEGER NOT NULL DEFAULT 1
             ) STRICT;
             INSERT INTO playlist_state (
                 singleton, loop_mode, sort_mode, is_loop_one, collection_revision
             ) VALUES (1, 'list', 'added', 0, 7);
             INSERT INTO playlists (
                 id, name, created_at, order_index, revision, is_protected
             ) VALUES ('favorites', 'Favorites', 1, 0, 1, 1);
             INSERT INTO playlist_entries (
                 id, playlist_id, path, order_index, added_at, created_at, updated_at,
                 record_version, last_modified_by_device_id, sync_status
             ) VALUES ('default-entry', 'default', '/media/default.mp4', 0, 1, 1, 1, 1, 'device-1', 0);",
        )
        .expect("create legacy playlist tables");

        migrate_schema_v3_to_v4(&mut conn).expect("resume legacy playlist migration");

        let (playlist_updated, state_updated): (i64, i64) = conn
            .query_row(
                "SELECT
                     (SELECT COUNT(*) FROM pragma_table_info('playlists') WHERE name = 'updated_at'),
                     (SELECT COUNT(*) FROM pragma_table_info('playlist_state') WHERE name = 'updated_at')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read migrated columns");
        assert_eq!(playlist_updated, 1);
        assert_eq!(state_updated, 1);

        let collection_revision: i64 = conn
            .query_row(
                "SELECT collection_revision FROM playlist_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("preserve collection state");
        assert_eq!(collection_revision, 7);

        let default_order_index: i64 = conn
            .query_row(
                "SELECT order_index FROM playlists WHERE id = 'default'",
                [],
                |row| row.get(0),
            )
            .expect("read migrated default playlist order");
        assert_eq!(default_order_index, 1);
    }

    #[test]
    fn unsupported_existing_schema_is_not_rebuilt() {
        let mut conn = memory_conn();
        reset_schema(&conn).expect("create old schema");
        conn.execute(
            "INSERT INTO app_installation_state (
                 singleton,
                 install_id,
                 device_id,
                 install_id_updated_at,
                 uuid_update_data,
                 updated_at
             )
             VALUES (1, ?1, ?2, 123, '{}', 456)",
            params![INSTALL_ID, DEVICE_ID],
        )
        .expect("insert installation state");
        conn.execute_batch("PRAGMA user_version = 1;")
            .expect("set old user version");

        let error = ensure_schema(&mut conn).expect_err("unsupported schema should fail");
        assert!(
            error.contains("refusing to rebuild an existing database"),
            "unexpected error: {error}"
        );

        let (install_id, device_id, install_id_updated_at): (String, String, i64) = conn
            .query_row(
                "SELECT install_id, device_id, install_id_updated_at
                 FROM app_installation_state
                 WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read preserved installation state");
        assert_eq!(install_id, INSTALL_ID);
        assert_eq!(device_id, DEVICE_ID);
        assert_eq!(install_id_updated_at, 123);
    }

    #[test]
    fn existing_db_without_installation_state_is_not_rebuilt() {
        let mut conn = memory_conn();
        reset_schema(&conn).expect("create old schema");
        conn.execute_batch("PRAGMA user_version = 1;")
            .expect("set old user version");

        let error = ensure_schema(&mut conn).expect_err("ensure schema should fail");
        assert!(
            error.contains("refusing to rebuild an existing database"),
            "unexpected error: {error}"
        );

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM app_installation_state", [], |row| {
                row.get(0)
            })
            .expect("count installation rows");
        assert_eq!(count, 0);
    }
}
