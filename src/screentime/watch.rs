//! Watch loop: Hyprland socket events, session tracking, daily JSON mirror.
//!
//! The old loop polled `hyprctl activewindow` every 2 seconds forever.
//! This listens on Hyprland's event socket instead and wakes on
//! `activewindow` events, with a 5s tick as a fallback (title changes
//! within a window do not always fire an event). That cuts the steady
//! state to one hyprctl spawn per window switch plus one per tick.

use super::db::{active_window, open_db, write_daily_json};
use super::time::now_secs;
use rusqlite::{Connection, params};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

type Session = Option<(i64, String, String)>; // (session_id, class, title)

fn transition(conn: &Connection, current: Session, aw: Option<(String, String)>) -> Session {
    let aw2 = aw.clone();
    match (current, aw) {
        (Some((id, cls, _)), Some((new_cls, new_title))) if cls == new_cls => {
            // same window: update end_time/duration
            let now = now_secs();
            let _ = conn.execute(
                "UPDATE sessions SET end_time = ?1, duration = ?1 - start_time WHERE id = ?2",
                params![now, id],
            );
            Some((id, new_cls, new_title))
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
                let _ = conn.execute(
                    "INSERT INTO sessions (app_class, title, start_time, end_time, duration) VALUES (?1, ?2, ?3, 0, 0)",
                    params![cls, title, now],
                );
                let last = conn.last_insert_rowid();
                Some((last, cls, title))
            } else {
                None
            }
        }
        (None, Some((cls, title))) => {
            let now = now_secs();
            let _ = conn.execute(
                "INSERT INTO sessions (app_class, title, start_time, end_time, duration) VALUES (?1, ?2, ?3, 0, 0)",
                params![cls, title, now],
            );
            let last = conn.last_insert_rowid();
            Some((last, cls, title))
        }
        (None, None) => None,
    }
}

/// Forward `activewindow` events from Hyprland's socket2 into the
/// channel; reconnects with a pause if the socket drops (compositor
/// restart). Only the arrival matters — the new window is read back
/// through hyprctl, so payload parsing stays out of the hot path.
fn event_listener(tx: mpsc::Sender<()>) {
    loop {
        let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default();
        let runtime =
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
        let sock = format!("{}/hypr/{}/.socket2.sock", runtime, sig);
        let sock = if Path::new(&sock).exists() {
            sock
        } else {
            format!("/tmp/hypr/{}/.socket2.sock", sig)
        };
        if Path::new(&sock).exists()
            && let Ok(stream) = UnixStream::connect(&sock)
        {
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if (line.starts_with("activewindow>>") || line.starts_with("activewindowv2>>"))
                    && tx.send(()).is_err()
                {
                    return;
                }
            }
        }
        thread::sleep(Duration::from_secs(2));
    }
}

pub fn watch() {
    let conn = match open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("screentime: cannot open db: {}", e);
            return;
        }
    };
    let (tx, rx) = mpsc::channel::<()>();
    thread::spawn(move || event_listener(tx));
    let mut current: Session = None;
    loop {
        // Wake on window switches, or every 5s to keep the current
        // session's duration and the daily JSON fresh.
        let _ = rx.recv_timeout(Duration::from_secs(5));
        current = transition(&conn, current, active_window());
        write_daily_json(&conn);
    }
}
