//! Apply: resolve selections per output, render what is missing,
//! write background.json and hand videos to mpvpaper.
//!
//! Port of apply() in legacy wallpaper.sh. The state writes happen
//! here; the shell draws stills from background.json it watches.

use super::catalog::is_video;
use crate::theme::state::write_atomic as write_atomic_state;
use super::select::wallpapers_ensure;
use super::state::{current_theme, generated_file, posters_dir, state_dir};
use crate::proc::{run, tool};
use std::path::Path;

fn magick() -> String { tool("MAGICK", "magick") }
fn ffmpeg() -> String { tool("FFMPEG", "ffmpeg") }
fn mpvpaper() -> String { tool("MPVPAPER", "mpvpaper") }
fn hyprctl() -> String { tool("HYPRCTL", "hyprctl") }
fn quickshell() -> String { tool("QUICKSHELL", "quickshell") }
fn sha256sum() -> String { tool("SHA256SUM", "sha256sum") }
fn stat_bin() -> String { tool("STAT", "stat") }
fn pkill() -> String { tool("PKILL", "pkill") }

/// A palette colour by the legacy fallback chain: the live
/// generation first, then the saved palette, then the preset.
pub(crate) fn colour(key: &str) -> String {
    let live = crate::theme::state::live_link().join("palette.json");
    if live.is_file()
        && let Ok(s) = std::fs::read_to_string(&live)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
            && let Some(c) = v.get(key).and_then(|x| x.as_str())
        {
            return c.to_string();
        }
    let saved = crate::theme::state::generated_file();
    if saved.is_file()
        && let Ok(s) = std::fs::read_to_string(&saved)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
            && let Some(c) = v.get(key).and_then(|x| x.as_str())
        {
            return c.to_string();
        }
    let theme = current_theme();
    let theme = if theme == "wallpaper" {
        crate::config::get_str(&["theme", "default"], "tokyo-night")
    } else {
        theme
    };
    crate::theme::state::theme_field(&theme, key)
}

fn output_names() -> Vec<String> {
    let (code, out, _) = run(&hyprctl(), &["monitors", "-j"]);
    if code != 0 {
        return Vec::new();
    }
    serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .map(|arr| arr.iter().filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

/// Output dimensions, falling back to 1920x1080 like legacy.
fn output_size(output: &str) -> String {
    if !output.is_empty() {
        let (code, out, _) = run(&hyprctl(), &["monitors", "-j"]);
        if code == 0
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&out)
        {
            for m in v.as_array().cloned().unwrap_or_default() {
                if m.get("name").and_then(|n| n.as_str()) == Some(output) {
                    let w = m.get("width").and_then(|w| w.as_i64()).unwrap_or(0);
                    let h = m.get("height").and_then(|h| h.as_i64()).unwrap_or(0);
                    if w > 0 && h > 0 {
                        return format!("{}x{}", w, h);
                    }
                }
            }
        }
    }
    format!(
        "{}x{}",
        std::env::var("WIDTH").unwrap_or_else(|_| "1920".to_string()),
        std::env::var("HEIGHT").unwrap_or_else(|_| "1080".to_string())
    )
}

fn selection_json(theme: &str, output: &str) -> Option<serde_json::Value> {
    wallpapers_ensure().ok();
    let content = std::fs::read_to_string(super::catalog::wallpapers_file()).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let theme_entry = value.get("themes")?.get(theme)?;
    let selected = if !output.is_empty() {
        theme_entry.get("outputs").and_then(|o| o.get(output)).or_else(|| theme_entry.get("fallback"))
    } else {
        theme_entry.get("fallback")
    };
    Some(selected.cloned().unwrap_or(serde_json::Value::Null))
}

/// (mode, path, video) for one theme+output, falling back to the
/// saved choice when there is no per-output selection (legacy
/// selection()).
fn selection(theme: &str, output: &str) -> (String, String, bool) {
    if let Some(sel) = selection_json(theme, output)
        && !sel.is_null()
    {
        let mode = sel.get("mode").and_then(|m| m.as_str()).unwrap_or("file").to_string();
        let path = sel.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string();
        let video = sel.get("video").and_then(|v| v.as_bool()).unwrap_or(false);
        return (mode, path, video);
    }
    let old = chosen();
    if old == "generated" || !Path::new(&old).is_file() {
        ("generated".to_string(), String::new(), false)
    } else {
        let video = is_video(&old);
        ("file".to_string(), old, video)
    }
}

fn stat_field(path: &Path, fmt: &str) -> String {
    let (code, out, _) = run(&stat_bin(), &["-Lc", fmt, &path.to_string_lossy()]);
    if code == 0 { out.trim().to_string() } else { "0".to_string() }
}

fn sha256_hex(input: &str) -> String {
    let (code, out, _) = crate::proc::run_with_stdin(&sha256sum(), &[], input);
    if code == 0 { out.split_whitespace().next().unwrap_or("").to_string() } else { String::new() }
}

fn poster_frame(path: &Path, w: &str, h: &str, out: &Path) -> bool {
    for seek in ["1", "0"] {
        let (code, _, _) = run(&ffmpeg(), &["-y", "-loglevel", "error", "-ss", seek, "-i", &path.to_string_lossy(), "-frames:v", "1", "-vf", &format!("scale={}:{}:force_original_aspect_ratio=increase,crop={}:{}", w, h, w, h), "-q:v", "2", &out.to_string_lossy()]);
        if code == 0 && std::fs::metadata(out).map(|m| m.len()).unwrap_or(0) > 0 {
            return true;
        }
    }
    false
}

/// A still from a video for surfaces that cannot play it (lock
/// screen, anything reading background.json). Keyed by path, mtime,
/// size and output dimensions, like legacy poster_for.
fn poster_for(path: &Path, output: &str) -> String {
    if !path.is_file() || !is_video(&path.to_string_lossy()) {
        return String::new();
    }
    let dims = output_size(output);
    let (w, h) = dims.split_once('x').unwrap_or(("1920", "1080"));
    let key = sha256_hex(&format!(
        "{}\n{}\n{}\n{}\n",
        path.display(),
        stat_field(path, "%Y"),
        stat_field(path, "%s"),
        dims
    ));
    if key.is_empty() {
        return String::new();
    }
    let posters = posters_dir();
    let poster = posters.join(format!("{}.jpg", key));
    if !poster.is_file() {
        let _ = std::fs::create_dir_all(&posters);
        let tmp = posters.join(format!(".poster-{}.tmp.jpg", std::process::id()));
        if poster_frame(path, w, h, &tmp) {
            let _ = std::fs::rename(&tmp, &poster);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    if poster.is_file() { poster.to_string_lossy().to_string() } else { String::new() }
}

/// The palette gradient for one output. Pure function of palette +
/// size, so a still-valid file (same size marker, newer than the
/// palette) is reused instead of re-rendering.
fn generate(output: &str) -> Result<(), String> {
    let file = generated_file(output);
    let size = output_size(output);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let palette = crate::theme::state::live_link().join("palette.json");
    if file.is_file()
        && std::fs::read_to_string(file.with_extension("png.size")).map(|s| s.trim() == size).unwrap_or(false)
        && palette.is_file()
        && std::fs::metadata(&file).ok().and_then(|m| m.modified().ok()).map(|t| t >= std::fs::metadata(&palette).and_then(|m| m.modified()).unwrap_or(t)).unwrap_or(false)
    {
        return Ok(());
    }
    let bg = colour("background");
    let surface = colour("surface");
    let primary = colour("primary");
    let tmp = file.with_extension(format!("generate-{}.tmp.png", std::process::id()));
    let (code, _, err) = run(&magick(), &[
        "-size", &size,
        &format!("gradient:{}-{}", bg, surface),
        "-size", &size,
        &format!("radial-gradient:none-{}", primary),
        "-alpha", "set", "-channel", "A", "-evaluate", "multiply", "0.10", "+channel",
        "-compose", "over", "-composite",
        "-depth", "8",
        &tmp.to_string_lossy(),
    ]);
    if code != 0 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("generate failed: {}", err.trim()));
    }
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    let _ = write_atomic_state(&file.with_extension("png.size"), &format!("{}\n", size));
    Ok(())
}

fn prune_poster_cache(background: &Path) {
    let Ok(content) = std::fs::read_to_string(background) else { return };
    let keep: Vec<String> = serde_json::from_str::<serde_json::Value>(&content)
        .ok()
        .map(|v| {
            let mut keep = Vec::new();
            if let Some(outputs) = v.get("outputs").and_then(|o| o.as_object()) {
                for entry in outputs.values() {
                    if let Some(p) = entry.get("poster").and_then(|p| p.as_str()) {
                        keep.push(p.to_string());
                    }
                }
            }
            if let Some(p) = v.get("fallback").and_then(|f| f.get("poster")).and_then(|p| p.as_str()) {
                keep.push(p.to_string());
            }
            keep
        })
        .unwrap_or_default();
    prune_cache(&posters_dir(), &keep);
}

fn prune_cache(dir: &Path, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if keep.iter().any(|k| Path::new(k) == p) {
            continue;
        }
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

fn prune_generated(outputs: &[String]) {
    let dir = super::state::generated_dir();
    if !dir.is_dir() {
        return;
    }
    let mut keep: Vec<String> = Vec::new();
    for output in outputs {
        let f = generated_file(output);
        keep.push(f.to_string_lossy().to_string());
        keep.push(f.with_extension("png.size").to_string_lossy().to_string());
    }
    let fallback = generated_file("");
    keep.push(fallback.to_string_lossy().to_string());
    keep.push(fallback.with_extension("png.size").to_string_lossy().to_string());
    prune_cache(&dir, &keep);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map(|e| e == "size").unwrap_or(false)
                && !p.with_extension("png").is_file()
            {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

pub(crate) fn apply() -> String {
    wallpapers_ensure().ok();
    let theme = current_theme();
    let _ = run(&pkill(), &["-x", "mpvpaper"]);
    let _ = std::fs::remove_file(state_dir().join("video-paused"));

    let outputs = output_names();
    let background = super::catalog::background_file();

    let (fallback_mode, fallback_path0, fallback_video0) = selection(&theme, "");
    let mut fallback_path = fallback_path0.clone();
    let mut fallback_video = fallback_video0;
    if fallback_mode == "generated" || !Path::new(&fallback_path).is_file() {
        let _ = generate("");
        let generated = generated_file("");
        fallback_path = if generated.is_file() { generated.to_string_lossy().to_string() } else { String::new() };
        fallback_video = false;
    }
    let fallback_poster = if fallback_video { poster_for(Path::new(&fallback_path), "") } else { String::new() };

    let mut rows: Vec<(String, String, bool, String)> = Vec::new();
    for output in &outputs {
        let (mode, path0, video0) = selection(&theme, output);
        let mut path = path0.clone();
        let mut video = video0;
        if mode == "generated" || !Path::new(&path).is_file() {
            let _ = generate(output);
            let generated = generated_file(output);
            path = if generated.is_file() { generated.to_string_lossy().to_string() } else { String::new() };
            video = false;
        }
        let poster = if video {
            let p = poster_for(Path::new(&path), output);
            if !output.is_empty() && Path::new(&path).is_file() {
                let _ = std::process::Command::new(mpvpaper())
                    .args(["-f", "-o", "no-audio loop", output, &path])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            p
        } else {
            String::new()
        };
        rows.push((output.clone(), path, video, poster));
    }
    if outputs.is_empty() {
        rows.push((String::new(), fallback_path.clone(), fallback_video, fallback_poster.clone()));
    }

    let mut map = serde_json::Map::new();
    for (output, path, video, poster) in &rows {
        map.insert(output.clone(), serde_json::json!({"path": path, "video": video, "poster": poster}));
    }
    let value = serde_json::json!({
        "outputs": serde_json::Value::Object(map),
        "fallback": {"path": fallback_path, "video": fallback_video, "poster": fallback_poster},
    });
    if write_atomic_state(&background, &serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())).is_err() {
        return "ERR could not write background.json".to_string();
    }

    prune_poster_cache(&background);
    prune_generated(&outputs);

    let marker = if fallback_video {
        format!("video\n{}\n", fallback_path)
    } else {
        format!("{}\n", fallback_path)
    };
    let _ = write_atomic_state(&state_dir().join("background"), &marker);
    let _ = run(&quickshell(), &["ipc", "call", "background", "reload"]);
    "OK".to_string()
}

/// The saved choice, like legacy chosen(): wallpapers.json
/// fallback first, then wallpaper.$theme, else "generated".
pub(crate) fn chosen() -> String {
    super::select::wallpapers_ensure().ok();
    let theme = current_theme();
    let wallpapers = super::catalog::wallpapers_file();
    if let Ok(content) = std::fs::read_to_string(&wallpapers)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(path) = value
            .get("themes")
            .and_then(|t| t.get(&theme))
            .and_then(|t| t.get("fallback"))
            .and_then(|f| f.get("path"))
            .and_then(|p| p.as_str())
        && !path.is_empty()
    {
        let mode = value
            .get("themes").and_then(|t| t.get(&theme))
            .and_then(|t| t.get("fallback"))
            .and_then(|f| f.get("mode"))
            .and_then(|m| m.as_str())
            .unwrap_or("file");
        return if mode == "generated" { "generated".to_string() } else { path.to_string() };
    }
    let file = state_dir().join(format!("wallpaper.{}", theme));
    if let Ok(s) = std::fs::read_to_string(&file) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    "generated".to_string()
}
