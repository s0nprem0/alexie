//! Screen time: daily totals, reset, JSON, panel.
//!
//! Split from a single 298-line screentime.rs:
//! - `time` — now/today/midnight/format
//! - `db` — paths, open/migrate, daily JSON mirror
//! - `watch` — polling loop block
//! `handle()` below is non-DB subcommands first, then DB-backed.

mod db;
mod time;
mod watch;

use crate::proc::run;
use db::open_db;
use rusqlite::params;
use std::env;
use std::thread;
use std::time::Duration;
use time::{format_seconds, today_start, today_string};

pub use watch::watch;

pub fn db_path() -> std::path::PathBuf { db::db_path() }
pub fn daily_file_path() -> std::path::PathBuf { db::daily_file_path() }

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("show");
    // Subcommands that do not need the DB come first, so a read-only
    // query like `path` or the shell's `panel` click does not create
    // (and migrate) the database as a side effect.
    match sub {
        "path" => return daily_file_path().to_string_lossy().to_string(),
        "db-path" => return db_path().to_string_lossy().to_string(),
        "watch" => return "Run `alexie screentime watch` directly (it blocks).".to_string(),
        "panel" => {
            // Match legacy screentime.sh panel: enable the bar widget
            // (idempotent, so layouts predating it gain it), then
            // toggle the screentime panel. The QML ScreenLayers /
            // Indicators call `alexie screentime panel` via runAlexie,
            // and this previously returned ERR unknown subcommand.
            let qs = env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string());
            let _ = run(&qs, &["ipc", "call", "bar", "enable", "bar.screentime"]);
            thread::sleep(Duration::from_millis(200));
            let (code, out, err) = run(&qs, &["ipc", "call", "panel", "toggle", "screentime"]);
            if code != 0 {
                return format!("ERR (exit {}) {}", code, err.trim());
            }
            let reply = out.trim();
            return if reply.is_empty() || reply == "ok" { "OK".to_string() } else { reply.to_string() };
        }
        _ => {}
    }
    // Anything else must be a subcommand that uses the DB. Reject unknown
    // words before opening it, so a typo neither creates the database nor
    // depends on the state directory being writable.
    match sub {
        "" | "show" | "json" | "reset" => {}
        _ => return format!("ERR unknown subcommand: {}", sub),
    }
    let conn = match open_db() {
        Ok(c) => c,
        Err(e) => return format!("ERR db: {}", e),
    };
    match sub {
        "" | "show" => {
            let ts = today_start();
            let mut stmt = match conn.prepare(
                "SELECT app_class, SUM(duration) FROM sessions WHERE start_time >= ?1 GROUP BY app_class ORDER BY SUM(duration) DESC LIMIT 8",
            ) {
                Ok(s) => s,
                Err(e) => return format!("ERR: {}", e),
            };
            let rows: Vec<(String, i64)> = stmt
                .query_map(params![ts], |r| Ok((r.get(0)?, r.get(1)?)))
                .map(|iter| iter.filter_map(|r| r.ok()).collect())
                .unwrap_or_default();
            let total: i64 = rows.iter().map(|(_, d)| *d).sum();
            if rows.is_empty() {
                return "No screen time recorded today. (run `alexie screentime watch`)".to_string();
            }
            let mut out = String::new();
            out.push_str(&format!("active: {}\n", format_seconds(total)));
            out.push('\n');
            let rows: Vec<Vec<String>> = rows.into_iter().map(|(app, secs)| vec![app, format_seconds(secs)]).collect();
            out.push_str(&crate::table::render(&["App", "Time"], &rows));
            out.trim_end().to_string()
        }
        "json" => {
            let ts = today_start();
            let mut stmt = match conn.prepare(
                "SELECT app_class, SUM(duration) FROM sessions WHERE start_time >= ?1 GROUP BY app_class",
            ) {
                Ok(s) => s,
                Err(e) => return format!("ERR: {}", e),
            };
            let mut apps = serde_json::Map::new();
            if let Ok(iter) = stmt.query_map(params![ts], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
                for (k, v) in iter.filter_map(|r| r.ok()) {
                    apps.insert(k, serde_json::json!(v));
                }
            }
            let total: i64 = apps.values().filter_map(|v| v.as_i64()).sum();
            let obj = serde_json::json!({"date": today_string(), "apps": apps, "active": total, "idle": 0});
            obj.to_string()
        }
        "reset" => {
            let ts = today_start();
            let _ = conn.execute("DELETE FROM sessions WHERE start_time >= ?1", params![ts]);
            "Screen time reset for today.".to_string()
        }
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_string_is_iso_date() {
        let day = today_string();
        // YYYY-MM-DD, 10 chars, dashes in the right places. Falls
        // back to empty only if `date` is missing (CI sandbox has it).
        if day.is_empty() {
            return;
        }
        assert_eq!(day.len(), 10, "got: {day}");
        assert_eq!(&day[4..5], "-");
        assert_eq!(&day[7..8], "-");
    }

    #[test]
    fn format_seconds_basic() {
        assert_eq!(format_seconds(0), "0m");
        assert_eq!(format_seconds(90), "1m");
        assert_eq!(format_seconds(3660), "1h 1m");
    }

    #[test]
    fn path_does_not_need_db() {
        // `path` returns today's daily JSON file (legacy semantics)
        // without opening the DB; `db-path` returns the DB file.
        let out = handle(&["path".to_string()]);
        assert!(out.ends_with(".json"), "got: {out}");
        assert!(out.contains("screentime"), "got: {out}");
        let db = handle(&["db-path".to_string()]);
        assert!(db.ends_with("screentime.db"), "got: {db}");
    }

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }
}
