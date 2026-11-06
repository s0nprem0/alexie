//! Time helpers: now, today, midnight, formatting.

use crate::proc::run;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub(crate) fn today_string() -> String {
    let (code, out, _) = run("date", &["+%F"]);
    if code == 0 { out.trim().to_string() } else { String::new() }
}

pub(crate) fn today_start() -> i64 {
    // local midnight as epoch seconds
    let (code, out, _) = run("date", &["-d", "today 00:00:00", "+%s"]);
    if code == 0 { out.trim().parse().unwrap_or(0) } else {
        let n = now_secs();
        n - (n % 86400)
    }
}

pub(crate) fn format_seconds(total: i64) -> String {
    let hours = total / 3600;
    let mins = (total % 3600) / 60;
    if hours > 0 { format!("{}h {}m", hours, mins) } else { format!("{}m", mins) }
}

