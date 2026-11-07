//! Backlight: read/set via brightnessctl, dim/restore tracking.

use super::config::brightnessctl;
use super::state::{dim_level_file, saved_brightness_file};
use crate::proc::run;

pub(crate) fn backlight_percent() -> Option<i64> {
    let (code, cur, _) = run(&brightnessctl(), &["-c", "backlight", "get"]);
    if code != 0 {
        return None;
    }
    let (code2, max, _) = run(&brightnessctl(), &["-c", "backlight", "max"]);
    if code2 != 0 {
        return None;
    }
    let cur: i64 = cur.trim().parse().ok()?;
    let max: i64 = max.trim().parse().ok()?;
    if max <= 0 {
        return None;
    }
    Some(cur * 100 / max)
}

pub(crate) fn set_backlight(pct: i64) {
    let _ = run(
        &brightnessctl(),
        &["-c", "backlight", "set", &format!("{}%", pct)],
    );
}

pub(crate) fn restore_brightness() {
    if let Ok(s) = std::fs::read_to_string(saved_brightness_file())
        && let Ok(v) = s.trim().parse::<i64>()
    {
        set_backlight(v);
    }
    let _ = std::fs::remove_file(saved_brightness_file());
    let _ = std::fs::remove_file(dim_level_file());
}

pub(crate) fn enter_dim(level: &str, target: i64) {
    if let Ok(s) = std::fs::read_to_string(dim_level_file())
        && s.trim() == level
    {
        return;
    }
    if !saved_brightness_file().exists()
        && let Some(cur) = backlight_percent()
    {
        let _ = std::fs::write(saved_brightness_file(), format!("{}\n", cur));
    }
    set_backlight(target);
    let _ = std::fs::write(dim_level_file(), format!("{}\n", level));
}
