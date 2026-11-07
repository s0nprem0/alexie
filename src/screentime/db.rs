//! DB: open/migrate, active window, daily JSON mirror.

use super::time::{now_secs, today_start, today_string};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::env;
use std::fs;
use std::process::Command;

pub(crate) fn state_dir() -> std::path::PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    std::path::PathBuf::from(state).join("anixie")
}

pub(crate) fn db_path() -> std::path::PathBuf {
    state_dir().join("screentime.db")
}

/// Today's daily JSON file, as legacy `screentime path` reports
/// ($state/YYYY-MM-DD.json). The shell (ScreentimeStore) reads this
/// file; the DB is the tracker's private store.
pub(crate) fn daily_file_path() -> std::path::PathBuf {
    state_dir()
        .join("screentime")
        .join(format!("{}.json", today_string()))
}

pub(crate) fn open_db() -> Result<Connection, rusqlite::Error> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = Connection::open(&path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app_class TEXT NOT NULL,
            title TEXT NOT NULL,
            start_time INTEGER NOT NULL,
            end_time INTEGER NOT NULL,
            duration INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_start ON sessions(start_time);
        CREATE INDEX IF NOT EXISTS idx_sessions_end ON sessions(end_time);",
    )?;
    // finalize any dangling sessions from a previous (killed) run
    let _ = conn.execute(
        "UPDATE sessions SET end_time = start_time, duration = 0 WHERE end_time = 0",
        [],
    );
    prune_old_sessions(&conn);
    Ok(conn)
}

/// Keep history bounded: sessions older than 30 days are deleted,
/// so the DB does not grow forever on a long-lived machine.
fn prune_old_sessions(conn: &Connection) {
    let cutoff = now_secs() - 30 * 24 * 60 * 60;
    let _ = conn.execute(
        "DELETE FROM sessions WHERE start_time < ?1",
        params![cutoff],
    );
}

pub(crate) fn active_window() -> Option<(String, String)> {
    let out = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let class = v.get("class")?.as_str()?.to_string();
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    if class.is_empty() {
        None
    } else {
        Some((class, title))
    }
}

pub(crate) fn write_daily_json(conn: &Connection) {
    let day = today_string();
    if day.is_empty() {
        return;
    }
    let dir = state_dir().join("screentime");
    let _ = fs::create_dir_all(&dir);
    let file = dir.join(format!("{}.json", day));
    let ts = today_start();
    let mut apps = serde_json::Map::new();
    let mut total: i64 = 0;
    if let Ok(mut stmt) = conn.prepare(
        "SELECT app_class, SUM(duration) FROM sessions WHERE start_time >= ?1 GROUP BY app_class",
    ) && let Ok(iter) = stmt.query_map(params![ts], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    }) {
        for (k, v) in iter.filter_map(|r| r.ok()) {
            apps.insert(k, serde_json::json!(v));
            total += v;
        }
    }
    let obj = serde_json::json!({"date": day, "apps": apps, "idle": 0, "active": total});
    let _ = fs::write(&file, obj.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_removes_only_sessions_older_than_30_days() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                app_class TEXT NOT NULL,
                title TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                end_time INTEGER NOT NULL,
                duration INTEGER NOT NULL
            );",
        )
        .unwrap();
        let now = now_secs();
        let old = now - 31 * 24 * 60 * 60;
        for (app, start) in [("old-app", old), ("new-app", now)] {
            conn.execute(
                "INSERT INTO sessions (app_class, title, start_time, end_time, duration) VALUES (?1, 't', ?2, ?2, 0)",
                rusqlite::params![app, start],
            )
            .unwrap();
        }
        prune_old_sessions(&conn);
        let count = |app: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE app_class = ?1",
                rusqlite::params![app],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(count("old-app"), 0, "31-day-old session should be pruned");
        assert_eq!(count("new-app"), 1, "recent session should be kept");
    }
}
