//! DB open/schema/migration + bookmarks/history/usage CRUD.

use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use super::paths::{cache_dir, db_path};
use super::types::{BookmarkItem, FileHistoryItem, WebHistoryItem};
use super::url::normalize_url;

pub(crate) fn init_db() -> Result<Connection, rusqlite::Error> {
    let db = db_path();
    if let Some(parent) = db.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = Connection::open(&db)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    init_schema(&conn)?;
    Ok(conn)
}

pub(crate) fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS bookmarks (
            url TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS file_history (
            path TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            timestamp INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS web_search_history (
            query TEXT COLLATE NOCASE NOT NULL,
            engine TEXT NOT NULL,
            url TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            PRIMARY KEY(query, engine)
        );
        CREATE TABLE IF NOT EXISTS app_usage (
            app_name TEXT PRIMARY KEY,
            count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_file_history_timestamp ON file_history(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_web_search_history_timestamp ON web_search_history(timestamp DESC);
        "#,
    )?;
    // Legacy migration.
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn migrate_if_needed(conn: &Connection, cache_dir: &Path) {
    // 1. bookmarks
    let bookmarks_path = cache_dir.join("bookmarks.json");
    if bookmarks_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&bookmarks_path)
            && let Ok(list) = serde_json::from_str::<Vec<BookmarkItem>>(&content)
        {
            let now = now_secs();
            let mut success = true;
            for (idx, item) in list.iter().enumerate() {
                let normalized = normalize_url(&item.url);
                if let Err(e) = conn.execute(
                    "INSERT OR IGNORE INTO bookmarks (url, name, created_at) VALUES (?1, ?2, ?3)",
                    params![normalized, item.name, now - idx as i64],
                ) {
                    eprintln!("Error migrating bookmark: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&bookmarks_path, cache_dir.join("bookmarks.json.migrated"));
        }
    }

    // 2. file history
    let file_history_path = cache_dir.join("file_history.json");
    if file_history_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&file_history_path)
            && let Ok(list) = serde_json::from_str::<Vec<FileHistoryItem>>(&content)
        {
            let mut success = true;
            for item in list {
                if let Err(e) = conn.execute(
                    "INSERT OR IGNORE INTO file_history (path, name, timestamp) VALUES (?1, ?2, ?3)",
                    params![item.path, item.name, item.timestamp],
                ) {
                    eprintln!("Error migrating file history: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&file_history_path, cache_dir.join("file_history.json.migrated"));
        }
    }

    // 3. web history
    let web_history_path = cache_dir.join("web_search_history.json");
    if web_history_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&web_history_path)
            && let Ok(list) = serde_json::from_str::<Vec<WebHistoryItem>>(&content)
        {
            let now = now_secs();
            let mut success = true;
            for (idx, item) in list.iter().enumerate() {
                if let Err(e) = conn.execute(
                    "INSERT OR IGNORE INTO web_search_history (query, engine, url, timestamp) VALUES (?1, ?2, ?3, ?4)",
                    params![item.query, item.engine, item.url, now - idx as i64],
                ) {
                    eprintln!("Error migrating web history: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&web_history_path, cache_dir.join("web_search_history.json.migrated"));
        }
    }

    // 4. usage
    let usage_path = cache_dir.join("app_usage.json");
    if usage_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&usage_path)
            && let Ok(map) = serde_json::from_str::<HashMap<String, u32>>(&content)
        {
            let mut success = true;
            for (app_name, count) in map {
                if let Err(e) = conn.execute(
                    "INSERT OR REPLACE INTO app_usage (app_name, count) VALUES (?1, ?2)",
                    params![app_name, count],
                ) {
                    eprintln!("Error migrating app usage: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&usage_path, cache_dir.join("app_usage.json.migrated"));
        }
    }
}

pub(crate) fn open_db() -> Result<Connection, rusqlite::Error> {
    let conn = init_db()?;
    let cache = cache_dir();
    let _ = fs::create_dir_all(&cache);
    migrate_if_needed(&conn, &cache);
    Ok(conn)
}

pub(crate) fn get_bookmarks(conn: &Connection) -> Vec<BookmarkItem> {
    let Ok(mut stmt) = conn.prepare("SELECT url, name FROM bookmarks ORDER BY created_at DESC") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| Ok(BookmarkItem { url: row.get(0)?, name: row.get(1)? })) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

pub(crate) fn add_bookmark(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    let normalized_url = normalize_url(url);
    let name = normalized_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or(&normalized_url)
        .to_string();
    conn.execute(
        "INSERT OR REPLACE INTO bookmarks (url, name, created_at) VALUES (?1, ?2, ?3)",
        params![normalized_url, name, now_secs()],
    )?;
    Ok(())
}

pub(crate) fn delete_bookmark(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM bookmarks WHERE url = ?1", params![normalize_url(url)])?;
    Ok(())
}

pub(crate) fn clear_bookmarks(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM bookmarks", [])?;
    Ok(())
}

pub(crate) fn get_file_history(conn: &Connection) -> Vec<FileHistoryItem> {
    let Ok(mut stmt) = conn.prepare("SELECT path, name, timestamp FROM file_history ORDER BY timestamp DESC LIMIT 30") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok(FileHistoryItem { path: row.get(0)?, name: row.get(1)?, timestamp: row.get(2)? })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

pub(crate) fn add_file_history(conn: &Connection, file_path: &str) -> Result<(), rusqlite::Error> {
    let name = Path::new(file_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file_path)
        .to_string();
    conn.execute(
        "INSERT OR REPLACE INTO file_history (path, name, timestamp) VALUES (?1, ?2, ?3)",
        params![file_path, name, now_secs()],
    )?;
    Ok(())
}

pub(crate) fn clear_file_history(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM file_history", [])?;
    Ok(())
}

pub(crate) fn get_web_history(conn: &Connection) -> Vec<WebHistoryItem> {
    let Ok(mut stmt) = conn.prepare("SELECT query, engine, url FROM web_search_history ORDER BY timestamp DESC LIMIT 20") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok(WebHistoryItem { query: row.get(0)?, engine: row.get(1)?, url: row.get(2)? })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

pub(crate) fn add_web_history(conn: &Connection, item: &WebHistoryItem) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO web_search_history (query, engine, url, timestamp) VALUES (?1, ?2, ?3, ?4)",
        params![item.query, item.engine, item.url, now_secs()],
    )?;
    Ok(())
}

pub(crate) fn clear_web_history(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM web_search_history", [])?;
    Ok(())
}

pub(crate) fn get_usage_map(conn: &Connection) -> HashMap<String, u32> {
    let Ok(mut stmt) = conn.prepare("SELECT app_name, count FROM app_usage") else {
        return HashMap::new();
    };
    let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))) else {
        return HashMap::new();
    };
    rows.filter_map(Result::ok).collect()
}

pub(crate) fn increment_app_usage(conn: &Connection, app_name: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO app_usage (app_name, count) VALUES (?1, 1) ON CONFLICT(app_name) DO UPDATE SET count = count + 1",
        params![app_name],
    )?;
    Ok(())
}
