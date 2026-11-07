//! Power state files + automation/preferred profiles.

use super::config::{ac_profile_default, auto_default, battery_profile_default};
use super::state_dir;
use std::path::PathBuf;

pub(crate) fn automation_file() -> PathBuf {
    state_dir().join("automation")
}
pub(crate) fn state_file_src(src: &str) -> PathBuf {
    state_dir().join(src)
}
pub(crate) fn saved_brightness_file() -> PathBuf {
    state_dir().join("brightness.saved")
}
pub(crate) fn dim_level_file() -> PathBuf {
    state_dir().join("brightness.level")
}
pub(crate) fn last_source_file() -> PathBuf {
    state_dir().join("last-source")
}

pub(crate) fn automation_enabled() -> bool {
    let p = automation_file();
    if p.exists()
        && let Ok(s) = std::fs::read_to_string(&p)
    {
        match s.trim() {
            "on" => return true,
            "off" => return false,
            _ => {}
        }
    }
    auto_default()
}

pub(crate) fn preferred(src: &str) -> String {
    let fallback = if src == "ac" {
        ac_profile_default()
    } else {
        battery_profile_default()
    };
    let p = state_file_src(src);
    if p.exists()
        && let Ok(s) = std::fs::read_to_string(&p)
    {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    fallback
}
