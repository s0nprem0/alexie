//! Recording state, monitor fps, status collection, indexing.

use std::fs;
use super::paths::{hyprctl, screenshot_dir, recording_dir};
use super::open::open;
use super::assets::{add_asset, check_deleted, list_assets};
use super::colors::{list_colors, list_ocr};
use super::types::MediaStatus;

/// Is a recorder running?
pub(crate) fn is_recording() -> bool {
    if crate::record::record_status() == "recording" {
        return true;
    }
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else { continue };
            if !name_str.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let comm_path = entry.path().join("comm");
            if fs::read_to_string(comm_path).is_ok_and(|comm| comm.trim() == "wf-recorder") {
                return true;
            }
        }
    }
    false
}

pub(crate) fn detect_monitor_fps() -> u32 {
    let (code, out, _) = crate::proc::run(&hyprctl(), &["monitors", "-j"]);
    if code != 0 {
        return 60;
    }
    serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .and_then(|arr| arr.into_iter().next())
        .and_then(|first| first.get("refreshRate").and_then(|r| r.as_f64()))
        .map(|rate| rate.round() as u32)
        .filter(|n| *n > 0)
        .unwrap_or(60)
}

pub(crate) fn collect_status() -> MediaStatus {
    let conn = match open() {
        Ok(c) => c,
        Err(_) => {
            return MediaStatus {
                is_recording: is_recording(),
                screenshot_dir: screenshot_dir(),
                recording_dir: recording_dir(),
                assets: Vec::new(),
                history: Vec::new(),
                colors: Vec::new(),
                monitor_fps: detect_monitor_fps(),
            };
        }
    };
    let _ = check_deleted(&conn);
    MediaStatus {
        is_recording: is_recording(),
        screenshot_dir: screenshot_dir(),
        recording_dir: recording_dir(),
        assets: list_assets(&conn, None, 50).unwrap_or_default(),
        history: list_ocr(&conn, 50).unwrap_or_default(),
        colors: list_colors(&conn, 24).unwrap_or_default(),
        monitor_fps: detect_monitor_fps(),
    }
}

/// Index a screenshot (best-effort).
pub(crate) fn index_screenshot(path: &str) {
    if path.is_empty() {
        return;
    }
    if let Ok(conn) = open()
        && let Err(e) = add_asset(&conn, "screenshot", path)
    {
        eprintln!("shot db: indexing {} failed: {}", path, e);
    }
}

/// Index a recording (best-effort).
pub(crate) fn index_recording(path: &str) {
    if path.is_empty() {
        return;
    }
    if let Ok(conn) = open()
        && let Err(e) = add_asset(&conn, "recording", path)
    {
        eprintln!("shot db: indexing {} failed: {}", path, e);
    }
}
