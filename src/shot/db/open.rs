//! DB open + schema + legacy migration.

use super::assets::add_asset;
use super::paths::{db_path, home_dir, thumbnails_dir};
use super::types::SCHEMA_VERSION;
use rusqlite::{Connection, OptionalExtension, params};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub fn open() -> Result<Connection, rusqlite::Error> {
    open_at(&db_path(), &thumbnails_dir())
}

pub(crate) fn open_at(db: &Path, thumbs: &Path) -> Result<Connection, rusqlite::Error> {
    if let Some(parent) = db.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::create_dir_all(thumbs);
    let conn = Connection::open(db)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    init_schema(&conn)?;
    Ok(conn)
}

pub(crate) fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS assets (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            asset_type      TEXT    NOT NULL CHECK(asset_type IN ('screenshot','recording')),
            source_path     TEXT    NOT NULL UNIQUE,
            thumbnail_path  TEXT    NOT NULL,
            created_at      INTEGER NOT NULL,
            deleted_at      INTEGER,
            file_size       INTEGER,
            width           INTEGER,
            height          INTEGER,
            duration_ms     INTEGER
        );
        CREATE TABLE IF NOT EXISTS colors (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            hex        TEXT    NOT NULL,
            picked_at  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ocr_history (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            detail      TEXT    NOT NULL,
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_assets_created_at ON assets(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_colors_picked_at  ON colors(picked_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ocr_created_at ON ocr_history(created_at DESC);
        "#,
    )?;

    let version: Option<i32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()?;
    let needs_migration = version.is_none() || version.unwrap_or(0) < SCHEMA_VERSION;

    if needs_migration {
        migrate_legacy(conn);
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (?1)",
            params![SCHEMA_VERSION],
        )?;
    }
    Ok(())
}

fn migrate_legacy(conn: &Connection) {
    let legacy_path = {
        let base = env::var("XDG_CACHE_HOME")
            .unwrap_or_else(|_| format!("{}/.cache", home_dir().display()));
        PathBuf::from(base).join("quickshell_media_history.json")
    };
    if !legacy_path.exists() {
        return;
    }
    let Ok(content) = fs::read_to_string(&legacy_path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };
    let Some(entries) = value.get("history").and_then(|v| v.as_array()) else {
        return;
    };

    for entry in entries {
        let Some(asset_type) = entry.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        if asset_type != "screenshot" && asset_type != "recording" {
            continue;
        }
        let Some(detail) = entry.get("detail").and_then(|v| v.as_str()) else {
            continue;
        };
        let _ = add_asset(conn, asset_type, detail);
    }

    let backup = legacy_path.with_extension("json.migrated");
    let _ = fs::rename(&legacy_path, &backup);
}
