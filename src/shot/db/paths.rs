//! Env helpers: tool overrides, state/cache dirs, clock.

use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn ffprobe() -> String {
    env::var("FFPROBE").unwrap_or_else(|_| "ffprobe".to_string())
}
pub(crate) fn ffmpeg() -> String {
    env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string())
}
pub(crate) fn hyprctl() -> String {
    env::var("HYPRCTL").unwrap_or_else(|_| "hyprctl".to_string())
}
pub(crate) fn hyprpicker() -> String {
    env::var("HYPRPICKER").unwrap_or_else(|_| "hyprpicker".to_string())
}

pub(crate) fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub(crate) fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}

pub(crate) fn cache_dir() -> PathBuf {
    let base =
        env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{}/.cache", home_dir().display()));
    PathBuf::from(base).join("anixie")
}

pub(crate) fn db_path() -> PathBuf {
    if let Ok(v) = env::var("SHOT_DB_PATH")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    if let Ok(v) = env::var("ASSETS_DB")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    state_dir().join("assets.db")
}

pub(crate) fn thumbnails_dir() -> PathBuf {
    if let Ok(v) = env::var("SHOT_THUMB_DIR")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    cache_dir().join("thumbnails")
}

pub(crate) fn screenshot_dir() -> String {
    env::var("SHOT_DIR").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/Pictures/Screenshots", home)
    })
}

pub(crate) fn recording_dir() -> String {
    if let Ok(v) = env::var("SCREENRECORD_DIR") {
        return v;
    }
    if let Ok(v) = env::var("XDG_VIDEOS_DIR") {
        return v;
    }
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{}/Videos", home)
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
