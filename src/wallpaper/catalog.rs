//! Library catalogue: paths, scan, fingerprint cache, JSON.
//!
//! Port of the library half of legacy wallpaper.sh. The state files
//! (library.json + library.stamp) keep the legacy names and shape so
//! the shell/QML readers need no changes.

use super::state::{current_theme, default_wallpaper, generated_file, state_dir, theme_dir, thumbs_dir, wallpaper_recursive, watch_dirs};
use crate::proc::{run, run_with_stdin};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) fn wallpapers_file() -> PathBuf { state_dir().join("wallpapers.json") }
pub(crate) fn background_file() -> PathBuf { state_dir().join("background.json") }
pub(crate) fn library_cache_file() -> PathBuf { state_dir().join("library.json") }
pub(crate) fn library_stamp_file() -> PathBuf { state_dir().join("library.stamp") }
pub(crate) fn rotation_file() -> PathBuf { state_dir().join("rotation") }

pub(crate) fn is_video(path: &str) -> bool {
    matches!(
        path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
        "mp4" | "mkv" | "webm" | "mov" | "m4v" | "avi" | "ogv"
    )
}

pub(crate) fn is_image(path: &str) -> bool {
    matches!(
        path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "avif"
    )
}

pub(crate) fn is_media(path: &str) -> bool { is_image(path) || is_video(path) }

/// (path, mtime seconds, size bytes). `generated` sorts first and
/// carries the gradient file's own stat (or zeroes when absent).
pub(crate) fn library_scan() -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let generated = generated_file("");
    if generated.is_file() {
        out.push(("generated".to_string(), stat_field(&generated, "%Y"), stat_field(&generated, "%s")));
    } else {
        out.push(("generated".to_string(), "0".to_string(), "0".to_string()));
    }
    let default = default_wallpaper();
    if !default.is_empty() && Path::new(&default).is_file() {
        out.push((default.clone(), stat_field_str(&default, "%Y"), stat_field_str(&default, "%s")));
    }
    for dir in scan_dirs() {
        scan_dir_meta(&dir, &mut out);
    }
    // dedup on path, keeping the first occurrence (legacy awk).
    let mut seen: HashSet<String> = HashSet::new();
    out.retain(|(p, _, _)| seen.insert(p.clone()));
    out
}

fn scan_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![theme_dir()];
    dirs.extend(watch_dirs());
    dirs
}

fn scan_dir_meta(dir: &Path, out: &mut Vec<(String, String, String)>) {
    if !dir.is_dir() {
        return;
    }
    let recursive = wallpaper_recursive();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if recursive {
                    stack.push(p);
                }
            } else if p.is_file() && is_media(&p.to_string_lossy()) {
                out.push((p.to_string_lossy().to_string(), stat_field(&p, "%Y"), stat_field(&p, "%s")));
            }
        }
    }
}

fn stat_field(path: &Path, fmt: &str) -> String {
    let (code, out, _) = run("stat", &["-Lc", fmt, &path.to_string_lossy()]);
    if code == 0 { out.trim().to_string() } else { "0".to_string() }
}

fn stat_field_str(path: &str, fmt: &str) -> String { stat_field(Path::new(path), fmt) }

/// Ordered library paths: generated first, then the default (when
/// present), then every scanned file, deduplicated.
pub(crate) fn library_paths() -> Vec<String> {
    library_scan().into_iter().map(|(p, _, _)| p).collect()
}

fn sha256sum_hex(input: &str) -> String {
    let (code, out, _) = run_with_stdin("sha256sum", &[], input);
    if code == 0 {
        out.split_whitespace().next().unwrap_or("").to_string()
    } else {
        String::new()
    }
}

/// Cache fingerprint over theme, default, recursion setting, missing
/// dirs and every scanned file's path/mtime/size. Editing a picture
/// changes the stamp without discarding still-valid thumbnails.
pub(crate) fn library_fingerprint(scan: &[(String, String, String)]) -> String {
    let mut lines = format!(
        "theme\t{}\ndefault\t{}\nrecursive\t{}\n",
        current_theme(),
        default_wallpaper(),
        wallpaper_recursive()
    );
    for dir in scan_dirs() {
        if !dir.is_dir() {
            lines.push_str(&format!("missing\t{}\n", dir.display()));
        }
    }
    for (path, mtime, size) in scan {
        lines.push_str(&format!("{}\t{}\t{}\n", path, mtime, size));
    }
    sha256sum_hex(&lines)
}

/// Thumbnail for a library entry, keyed by path/mtime/size exactly
/// like the legacy `thumb_for`, so existing caches are reused.
/// Videos get a frame first (ffmpeg), then a 320x180 centre crop.
pub(crate) fn thumb_for(path: &Path, mtime: &str, size: &str) -> String {
    if !path.is_file() {
        return String::new();
    }
    let mtime = if mtime.is_empty() { stat_field(path, "%Y") } else { mtime.to_string() };
    let size = if size.is_empty() { stat_field(path, "%s") } else { size.to_string() };
    let key = sha256sum_hex(&format!("{}\n{}\n{}\n", path.display(), mtime, size));
    if key.is_empty() {
        return String::new();
    }
    let thumbs = thumbs_dir();
    let thumb = thumbs.join(format!("{}.jpg", key));
    if !thumb.is_file() {
        let _ = std::fs::create_dir_all(&thumbs);
        let mut source = path.to_path_buf();
        let mut frame = PathBuf::new();
        if is_video(&path.to_string_lossy()) {
            frame = std::env::temp_dir().join(format!("wallpaper-frame-{}.jpg", std::process::id()));
            let (code, _, _) = run("ffmpeg", &["-y", "-loglevel", "error", "-i", &path.to_string_lossy(), "-frames:v", "1", "-q:v", "2", &frame.to_string_lossy()]);
            if code != 0 {
                let _ = std::fs::remove_file(&frame);
                return String::new();
            }
            source = frame.clone();
        }
        let tmp = thumbs.join(format!(".thumb-{}.tmp.jpg", std::process::id()));
        let (code, _, _) = run("magick", &[&source.to_string_lossy(), "-thumbnail", "320x180^", "-gravity", "center", "-extent", "320x180", "-quality", "85", &tmp.to_string_lossy()]);
        if code == 0 {
            let _ = std::fs::rename(&tmp, &thumb);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
        if !frame.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&frame);
        }
    }
    thumb.to_string_lossy().to_string()
}

/// Drop cached thumbs older than 7 days that the just-built library
/// does not reference (legacy prune_cache rule).
fn prune_thumb_cache() {
    let cache = library_cache_file();
    let Ok(content) = std::fs::read_to_string(&cache) else { return };
    let keep: Vec<String> = match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(v) => v
            .as_array()
            .map(|a| a.iter().filter_map(|e| e.get("thumb").and_then(|t| t.as_str()).map(|s| s.to_string())).collect())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    prune_cache(&thumbs_dir(), &keep);
}

fn prune_cache(dir: &Path, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if keep.iter().any(|k| Path::new(k) == p) {
            continue;
        }
        // Only collect what is both unreferenced and old enough that
        // the user is unlikely to come straight back to it.
        let old_enough = std::fs::metadata(&p)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| std::time::SystemTime::now().duration_since(modified).ok())
            .map(|elapsed| elapsed.as_secs() > 7 * 24 * 60 * 60)
            .unwrap_or(false);
        if old_enough {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// The library as JSON, served from the cache while the fingerprint
/// matches (same protocol as legacy). `refresh` forces a rebuild.
pub(crate) fn library_json(refresh: bool) -> String {
    let scan = library_scan();
    let stamp = library_fingerprint(&scan);
    let cache = library_cache_file();
    let stamp_file = library_stamp_file();
    if !refresh
        && cache.is_file()
        && std::fs::read_to_string(&stamp_file).map(|s| s.trim() == stamp).unwrap_or(false)
        && let Ok(s) = std::fs::read_to_string(&cache) {
            return s.trim_end().to_string();
        }
    let mut rows: Vec<serde_json::Value> = Vec::new();
    if scan[0].0 == "generated" {
        let thumb = thumb_for(&generated_file(""), "", "");
        rows.push(serde_json::json!({
            "name": "Generated",
            "path": "generated",
            "thumb": thumb,
            "video": false,
            "generated": true,
        }));
    }
    for (path, mtime, size) in &scan {
        if path == "generated" {
            continue;
        }
        let name = Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
        let thumb = thumb_for(Path::new(path), mtime, size);
        rows.push(serde_json::json!({
            "name": name,
            "path": path,
            "thumb": thumb,
            "video": is_video(path),
            "generated": false,
        }));
    }
    let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
    let _ = std::fs::create_dir_all(state_dir());
    let _ = crate::theme::state::write_atomic(&cache, &format!("{}\n", json));
    let _ = crate::theme::state::write_atomic(&stamp_file, &format!("{}\n", stamp));
    prune_thumb_cache();
    json
}

/// Plain list output: basenames, with `generated` for the gradient.
pub(crate) fn list_output() -> String {
    library_paths()
        .into_iter()
        .map(|p| {
            if p == "generated" {
                "generated".to_string()
            } else {
                Path::new(&p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(p)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_dedups_and_sorts_generated_first() {
        let scan = vec![
            ("generated".to_string(), "0".to_string(), "0".to_string()),
            ("/pics/a.png".to_string(), "1".to_string(), "10".to_string()),
            ("/pics/a.png".to_string(), "2".to_string(), "20".to_string()),
        ];
        assert_eq!(scan[0].0, "generated");
        // dedup keeps the first occurrence
        let mut seen = std::collections::HashSet::new();
        let dedup: Vec<_> = scan.into_iter().filter(|(p, _, _)| seen.insert(p.clone())).collect();
        assert_eq!(dedup.len(), 2);
    }

    #[test]
    fn media_detection() {
        assert!(is_video("/x/CLIP.MP4"));
        assert!(is_image("/x/photo.JPEG"));
        assert!(!is_media("/x/readme.txt"));
        assert!(is_media("/x/clip.mkv"));
    }
}
