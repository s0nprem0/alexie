//! Env helpers: tool overrides, db/cache paths.

use std::env;
use std::path::PathBuf;

pub(crate) fn fd() -> String {
    env::var("FD").unwrap_or_else(|_| "fd".to_string())
}
pub(crate) fn fzf() -> String {
    env::var("FZF").unwrap_or_else(|_| "fzf".to_string())
}
pub(crate) fn xdg_open() -> String {
    env::var("XDG_OPEN").unwrap_or_else(|_| "xdg-open".to_string())
}
pub(crate) fn hyprctl() -> String {
    env::var("HYPRCTL").unwrap_or_else(|_| "hyprctl".to_string())
}
pub(crate) fn thunar() -> String {
    env::var("THUNAR").unwrap_or_else(|_| "thunar".to_string())
}

pub(crate) fn home_dir_str() -> String {
    env::var("HOME").unwrap_or_default()
}

pub(crate) fn db_path() -> PathBuf {
    if let Ok(v) = env::var("APPS_DB")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie").join("launcher.db")
}

pub(crate) fn cache_dir() -> PathBuf {
    if let Ok(v) = env::var("APPS_CACHE_DIR")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    let base = env::var("XDG_CACHE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.cache", home)
    });
    PathBuf::from(base).join("anixie").join("launcher")
}
