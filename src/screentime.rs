use crate::proc::run;
use rusqlite::{Connection, params};
use serde_json::Value;
use std::env;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn state_dir() -> std::path::PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    std::path::PathBuf::from(state).join("anixie")
}

pub fn db_path() -> std::path::PathBuf {
    state_dir().join("screentime.db")
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn today_start() -> i64 {
    // local midnight as epoch seconds
    let (code, out, _) = run("date", &["-d", "today 00:00:00", "+%s"]);
    if code == 0 { out.trim().parse().unwrap_or(0) } else {
        let n = now_secs();
        n - (n % 86400)
    }
}

fn open_db() -> Result<Connection, rusqlite::Error> {
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
    let _ = conn.execute("UPDATE sessions SET end_time = start_time, duration = 0 WHERE end_time = 0", []);
    Ok(conn)
}

fn active_window() -> Option<(String, String)> {
    let out = Command::new("hyprctl").args(["activewindow", "-j"]).output().ok()?;
    if !out.status.success() { return None; }
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let class = v.get("class")?.as_str()?.to_string();
    let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if class.is_empty() { None } else { Some((class, title)) }
}

fn format_seconds(total: i64) -> String {
    let hours = total / 3600;
    let mins = (total % 3600) / 60;
    if hours > 0 { format!("{}h {}m", hours, mins) } else { format!("{}m", mins) }
}

fn write_daily_json(conn: &Connection) {
    let day = {
        let (code, out, _) = run("date", &["+%F"]);
        if code == 0 { out.trim().to_string() } else { String::new() }
    };
    if day.is_empty() { return; }
    let dir = state_dir().join("screentime");
    let _ = fs::create_dir_all(&dir);
    let file = dir.join(format!("{}.json", day));
    let ts = today_start();
    let mut apps = serde_json::Map::new();
    let mut total: i64 = 0;
    if let Ok(mut stmt) = conn.prepare(
        "SELECT app_class, SUM(duration) FROM sessions WHERE start_time >= ?1 GROUP BY app_class",
    ) {
        if let Ok(iter) = stmt.query_map(params![ts], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
            for (k, v) in iter.filter_map(|r| r.ok()) {
                apps.insert(k, serde_json::json!(v));
                total += v;
            }
        }
    }
    let obj = serde_json::json!({"date": day, "apps": apps, "idle": 0, "active": total});
    let _ = fs::write(&file, obj.to_string());
}

pub fn watch() {
    let conn = match open_db() {
        Ok(c) => c,
        Err(e) => { eprintln!("screentime: cannot open db: {}", e); return; }
    };
    let mut current: Option<(i64, String, String)> = None; // (session_id, class, title)
    loop {
        let aw = active_window();
        let aw2 = aw.clone();
        match (current.take(), aw) {
            (Some((id, cls, _)), Some((new_cls, new_title))) if cls == new_cls => {
                // same window: update end_time/duration
                let now = now_secs();
                let _ = conn.execute(
                    "UPDATE sessions SET end_time = ?1, duration = ?1 - start_time WHERE id = ?2",
                    params![now, id],
                );
                current = Some((id, new_cls, new_title));
            }
            (Some((id, _, _)), _) => {
                // finalize previous
                let now = now_secs();
                let _ = conn.execute(
                    "UPDATE sessions SET end_time = CASE WHEN end_time = 0 THEN ?1 ELSE end_time END, duration = CASE WHEN end_time = 0 THEN ?1 - start_time ELSE duration END WHERE id = ?2",
                    params![now, id],
                );
                // start new if present
                if let Some((cls, title)) = aw2 {
                    let now = now_secs();
                    if let Ok(id) = conn.execute(
                        "INSERT INTO sessions (app_class, title, start_time, end_time, duration) VALUES (?1, ?2, ?3, 0, 0)",
                        params![cls, title, now],
                    ) {
                        let _ = id;
                    }
                    // retrieve last id
                    let last = conn.last_insert_rowid();
                    current = Some((last, cls, title));
                } else {
                    current = None;
                }
            }
            (None, Some((cls, title))) => {
                let now = now_secs();
                let _ = conn.execute(
                    "INSERT INTO sessions (app_class, title, start_time, end_time, duration) VALUES (?1, ?2, ?3, 0, 0)",
                    params![cls, title, now],
                );
                let last = conn.last_insert_rowid();
                current = Some((last, cls, title));
            }
            (None, None) => {
                current = None;
            }
        }
        write_daily_json(&conn);
        thread::sleep(Duration::from_secs(2));
    }
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("show");
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
            let obj = serde_json::json!({"date": "", "apps": apps, "active": total, "idle": 0});
            obj.to_string()
        }
        "reset" => {
            let ts = today_start();
            let _ = conn.execute("DELETE FROM sessions WHERE start_time >= ?1", params![ts]);
            "Screen time reset for today.".to_string()
        }
        "path" => db_path().to_string_lossy().to_string(),
        "watch" => "Run `alexie screentime watch` directly (it blocks).".to_string(),
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}
