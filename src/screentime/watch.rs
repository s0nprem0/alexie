//! Watch loop: poll hyprctl, maintain sessions, mirror daily JSON.

use super::db::{active_window, open_db, write_daily_json};
use super::time::now_secs;
use rusqlite::params;
use std::thread;
use std::time::Duration;

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

