//! Selections: save/use/record/add for wallpapers.json.
//!
//! Port of the selection half of legacy wallpaper.sh. Apply (the
//! rendering/handoff half) still delegates until it is ported.

use super::catalog::{is_video, library_cache_file, library_stamp_file};
use super::state::{current_theme, state_dir, theme_dir};
use crate::theme::state::write_atomic;
use std::path::{Path, PathBuf};

fn wallpapers_file() -> PathBuf { super::catalog::wallpapers_file() }

pub(crate) fn wallpapers_ensure() -> Result<(), String> {
    let p = wallpapers_file();
    if std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) == 0 {
        write_atomic(&p, "{\"version\":1,\"themes\":{}}\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Hold the selection lock (flock, 30s wait) like legacy
/// wallpaper_lock. Returns an error string when the lock cannot be
/// taken, so callers show the same message.
pub(crate) struct SelectionLock(std::fs::File);

impl SelectionLock {
    pub(crate) fn acquire() -> Result<Self, String> {
        let lock_path = state_dir().join(".wallpapers.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| "wallpaper: could not lock the selection state".to_string())?;
        unsafe {
            use std::os::unix::io::AsRawFd;
            let mut deadline = 0;
            loop {
                let rc = libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB);
                if rc == 0 {
                    break;
                }
                if deadline >= 30 {
                    return Err("wallpaper: could not lock the selection state".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
                deadline += 1;
            }
        }
        Ok(SelectionLock(file))
    }
}

impl Drop for SelectionLock {
    fn drop(&mut self) {
        unsafe {
            use std::os::unix::io::AsRawFd;
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn read_wallpapers() -> serde_json::Value {
    wallpapers_ensure().ok();
    std::fs::read_to_string(wallpapers_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"version": 1, "themes": {}}))
}

fn write_wallpapers(value: &serde_json::Value) -> Result<(), String> {
    write_atomic(&wallpapers_file(), &serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
        .map_err(|e| e.to_string())
}

/// Save one selection: per-output when `output` is non-empty
/// (fallback untouched), else the fallback (per-output map cleared)
/// plus the legacy `wallpaper.$theme` reminder file.
pub(crate) fn save_selection(mode: &str, output: &str, path: &str, video: bool) -> Result<(), String> {
    let _lock = SelectionLock::acquire()?;
    wallpapers_ensure()?;
    let theme = current_theme();
    let mut value = read_wallpapers();
    let themes = value
        .as_object_mut()
        .and_then(|o| o.entry("themes").or_insert_with(|| serde_json::json!({})).as_object_mut())
        .ok_or("wallpaper: could not update selections".to_string())?;
    let theme_entry = themes
        .entry(theme.clone())
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("wallpaper: could not update selections".to_string())?;
    let selection = serde_json::json!({"mode": mode, "path": path, "video": video});
    if !output.is_empty() {
        let outputs = theme_entry
            .entry("outputs".to_string())
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or("wallpaper: could not update selections".to_string())?;
        outputs.insert(output.to_string(), selection);
    } else {
        theme_entry.insert("fallback".to_string(), selection);
        theme_entry.insert("outputs".to_string(), serde_json::json!({}));
    }
    write_wallpapers(&value)?;
    if output.is_empty() {
        let reminder = if mode == "generated" { "generated".to_string() } else { path.to_string() };
        let _ = write_atomic(&state_dir().join(format!("wallpaper.{}", theme)), &format!("{}\n", reminder));
    }
    Ok(())
}

/// Resolve a use/record target into (mode, path, video), checking
/// the file exists like legacy. `generated` is a mode, not a path.
fn resolve_target(target: &str) -> Result<(String, String, bool), String> {
    if target == "generated" {
        return Ok(("generated".to_string(), String::new(), false));
    }
    let path = Path::new(target);
    if !path.is_file() {
        return Err(format!("wallpaper: no such file: {}", target));
    }
    let canonical = path.canonicalize().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| target.to_string());
    let video = is_video(&canonical);
    Ok(("file".to_string(), canonical, video))
}

/// Record a selection without applying (what `theme wallpaper`
/// uses, so applying later does not re-record over the choice).
pub(crate) fn record_selection(target: &str, output: &str) -> String {
    match resolve_target(target) {
        Ok((mode, path, video)) => match save_selection(&mode, output, &path, video) {
            Ok(()) => "OK".to_string(),
            Err(e) => format!("ERR {}", e),
        },
        Err(e) => format!("ERR {}", e),
    }
}

/// Use a wallpaper now: save, derive a palette through theme when
/// the live theme is `wallpaper`, then apply.
pub(crate) fn use_wallpaper(target: &str, output: &str) -> String {
    wallpapers_ensure().ok();
    // Backups so a failed palette derivation can roll back.
    let _ = std::fs::copy(wallpapers_file(), state_dir().join(".wallpapers.previous"));
    let legacy = state_dir().join(format!("wallpaper.{}", current_theme()));
    let legacy_prev = state_dir().join(".wallpaper.previous");
    if legacy.is_file() {
        let _ = std::fs::copy(&legacy, &legacy_prev);
    } else {
        let _ = std::fs::remove_file(&legacy_prev);
    }

    let (mode, path, video) = match resolve_target(target) {
        Ok(r) => r,
        Err(e) => return format!("ERR {}", e),
    };
    if let Err(e) = save_selection(&mode, output, &path, video) {
        return format!("ERR {}", e);
    }

    if target != "generated" && current_theme() == "wallpaper" {
        // The selection is already saved: let theme derive the
        // palette without recording a second, global choice on
        // top of an output-specific one.
        let result = crate::theme::engine::generate::wallpaper_theme_recorded(target, "", true);
        if result.starts_with("ERR") {
            let _ = std::fs::copy(state_dir().join(".wallpapers.previous"), wallpapers_file());
            if legacy_prev.is_file() {
                let _ = std::fs::copy(&legacy_prev, &legacy);
            } else {
                let _ = std::fs::remove_file(&legacy);
            }
            return "ERR wallpaper: could not derive a palette; keeping the previous theme".to_string();
        }
        return format!("OK wallpaper: {}", target);
    }

    super::apply::apply()
}

fn clear_library_cache() {
    let _ = std::fs::remove_file(library_cache_file());
    let _ = std::fs::remove_file(library_stamp_file());
}

/// Copy a picture into the theme's backgrounds folder and use it.
pub(crate) fn add_file(file: &str) -> String {
    let src = Path::new(file);
    if !src.is_file() {
        return format!("ERR wallpaper: no such file: {}", file);
    }
    clear_library_cache();
    let dir = theme_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return format!("ERR {}", e);
    }
    let dst = dir.join(src.file_name().unwrap_or_default());
    if src.canonicalize().ok() != dst.canonicalize().ok()
        && let Err(e) = std::fs::copy(src, &dst) {
            return format!("ERR {}", e);
        }
    use_wallpaper(&dst.to_string_lossy(), "")
}

/// Copy every media file in a folder into the theme's folder.
pub(crate) fn add_dir(dir: &str) -> String {
    let src = Path::new(dir);
    if !src.is_dir() {
        return format!("ERR wallpaper: no such folder: {}", dir);
    }
    clear_library_cache();
    let dest = theme_dir();
    if let Err(e) = std::fs::create_dir_all(&dest) {
        return format!("ERR {}", e);
    }
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && super::catalog::is_media(&p.to_string_lossy())
                && std::fs::copy(&p, dest.join(p.file_name().unwrap_or_default())).is_ok() {
                    n += 1;
                }
        }
    }
    format!("Added {} background(s) to {}", n, current_theme())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_target_rejects_missing_files() {
        assert!(resolve_target("/nonexistent/picture.jpg").is_err());
        let (mode, path, video) = resolve_target("generated").unwrap();
        assert_eq!(mode, "generated");
        assert!(path.is_empty());
        assert!(!video);
    }
}
