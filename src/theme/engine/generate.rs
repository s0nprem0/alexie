use super::super::state::{
    generated_file, saved_wallpaper_mode, state_dir, wallpaper_mode_file, write_atomic,
};
use super::apps::{run_hooks, tell_apps};
use super::color::{palette_fix_value, palette_ok_file, palette_ok_value};
use super::materialise::{StateLock, materialise};
use crate::proc::{run, tool};
use std::env;
use std::path::Path;

fn matugen_bin() -> String {
    tool("MATUGEN", "matugen")
}
fn magick_bin() -> String {
    tool("MAGICK", "magick")
}
fn ffmpeg_bin() -> String {
    tool("FFMPEG", "ffmpeg")
}

// ---------- matugen palette transform (port of palette.jq) ----------

fn mc(m: &serde_json::Value, name: &str) -> String {
    m.get("colors")
        .and_then(|c| c.get(name))
        .and_then(|d| d.get("default"))
        .and_then(|col| col.get("color"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn matugen_to_palette(m: &serde_json::Value, fallback: &str) -> serde_json::Value {
    let mode = m.get("mode").and_then(|v| v.as_str()).unwrap_or(fallback);
    let mode = if mode == "light" || mode == "dark" {
        mode.to_string()
    } else {
        "dark".to_string()
    };
    let (success, yellow) = if mode == "light" {
        ("#2f7d32", "#9a6700")
    } else {
        ("#a6e3a1", "#f0c674")
    };
    let (bg_dark, bg_light) = if mode == "light" {
        (
            mc(m, "surface_container_highest"),
            mc(m, "surface_container_lowest"),
        )
    } else {
        (
            mc(m, "surface_container_lowest"),
            mc(m, "surface_container_highest"),
        )
    };
    let mut obj = serde_json::Map::new();
    let mut ins = |k: &str, v: String| {
        obj.insert(k.to_string(), serde_json::json!(v));
    };
    ins("background", mc(m, "background"));
    ins("surface", mc(m, "surface_container"));
    ins("surfaceContainer", mc(m, "surface_container_high"));
    ins("surfaceVariant", mc(m, "surface_variant"));
    ins("foreground", mc(m, "on_background"));
    ins("fgLight", mc(m, "on_surface_variant"));
    ins("bgDark", bg_dark);
    ins("bgLight", bg_light);
    ins("muted", mc(m, "on_surface_variant"));
    ins("outline", mc(m, "outline"));
    ins("primary", mc(m, "primary"));
    ins("accent", mc(m, "tertiary"));
    ins("accentAlt", mc(m, "secondary"));
    ins("secondary", mc(m, "secondary"));
    ins("tertiary", mc(m, "tertiary"));
    ins("success", success.to_string());
    ins("yellow", yellow.to_string());
    ins("danger", mc(m, "error"));
    ins("error", mc(m, "error"));
    ins("focus", mc(m, "primary"));
    ins("selectionBg", mc(m, "primary"));
    ins("selectionFg", mc(m, "on_primary"));
    ins("hover", mc(m, "surface_container_high"));
    ins("active", mc(m, "surface_container_highest"));
    ins("scrim", "#00000080".to_string());
    // ansi0..15, matching palette.jq
    let ansi = [
        mc(m, "background"),
        mc(m, "error"),
        success.to_string(),
        yellow.to_string(),
        mc(m, "primary"),
        mc(m, "secondary"),
        mc(m, "tertiary"),
        mc(m, "on_background"),
        mc(m, "on_surface_variant"),
        mc(m, "error"),
        success.to_string(),
        yellow.to_string(),
        mc(m, "primary"),
        mc(m, "secondary"),
        mc(m, "tertiary"),
        mc(m, "on_background"),
    ];
    for (i, c) in ansi.iter().enumerate() {
        ins(&format!("ansi{}", i), c.clone());
    }
    obj.insert("_mode".to_string(), serde_json::json!(mode));
    serde_json::Value::Object(obj)
}

// ---------- generate / wallpaper (port of generate/wallpaper_theme) ----------

fn matugen_version() -> String {
    let (code, out, _) = run(&matugen_bin(), &["--version"]);
    if code == 0 {
        out.lines().next().unwrap_or("").to_string()
    } else {
        String::new()
    }
}

fn file_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn generate_palette(wp: &str, requested: &str) -> Result<(), String> {
    std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
    let requested = match requested {
        "" | "auto" | "smart" => "smart",
        "light" | "dark" => requested,
        _ => return Err("ERR mode must be auto, light or dark".to_string()),
    };
    let wp_path = if wp.is_empty() {
        crate::wallpaper::handle(&["current".to_string()])
    } else {
        wp.to_string()
    };
    if wp_path.is_empty() || !Path::new(&wp_path).is_file() {
        return Err("ERR no picture to read a palette from — set one with 'wallpaper set FILE', or name one: 'theme wallpaper FILE'".to_string());
    }
    let wallpaper_png = state_dir().join("wallpaper.png");
    if let (Ok(a), Ok(b)) = (
        std::fs::canonicalize(&wp_path),
        std::fs::canonicalize(&wallpaper_png),
    ) && a == b
    {
        return Err("ERR the current background is generated from the palette; set a picture with 'wallpaper set FILE' first".to_string());
    }

    let mtime = file_mtime(Path::new(&wp_path));
    let key_material = format!(
        "{}\n{}\n{}\n{}\n{}",
        wp_path,
        mtime,
        requested,
        "1",
        matugen_version()
    );
    let key = {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        key_material.hash(&mut h);
        format!("{:016x}", h.finish())
    };
    let cache_dir = state_dir().join("cache");
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let cache = cache_dir.join(format!("palette-{}.json", key));
    // prune old cache entries (>30 days)
    if let Ok(read) = std::fs::read_dir(&cache_dir) {
        for entry in read.flatten() {
            if let Ok(m) = entry.metadata()
                && let Ok(modified) = m.modified()
                && modified
                    .elapsed()
                    .map(|d| d.as_secs() > 30 * 86400)
                    .unwrap_or(false)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    if cache.exists() && palette_ok_file(&cache) {
        let ok_mode = std::fs::read_to_string(&cache)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| {
                v.get("_mode")
                    .and_then(|m| m.as_str())
                    .map(|s| s == "dark" || s == "light")
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if ok_mode {
            std::fs::copy(&cache, generated_file()).map_err(|e| e.to_string())?;
            return Ok(());
        }
        let _ = std::fs::remove_file(&cache);
    }

    let ext = Path::new(&wp_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut source = wp_path.clone();
    if matches!(
        ext.as_str(),
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "ogv"
    ) {
        let frame = cache_dir.join(format!("frame-{}.jpg", key));
        if !frame.exists() {
            let (code, _, err) = run(
                &ffmpeg_bin(),
                &[
                    "-y",
                    "-loglevel",
                    "error",
                    "-i",
                    &wp_path,
                    "-frames:v",
                    "1",
                    "-q:v",
                    "2",
                    frame.to_string_lossy().as_ref(),
                ],
            );
            if code != 0 {
                return Err(format!(
                    "ERR could not read a frame from {}: {}",
                    wp_path,
                    err.trim()
                ));
            }
        }
        source = frame.to_string_lossy().to_string();
    }

    let (sat_code, sat_out, _) = run(
        &magick_bin(),
        &[
            &source,
            "-colorspace",
            "HSL",
            "-channel",
            "G",
            "-separate",
            "-format",
            "%[fx:mean]",
            "info:",
        ],
    );
    let saturation: f64 = if sat_code == 0 {
        sat_out.trim().parse().unwrap_or(1.0)
    } else {
        1.0
    };
    let mono = saturation < 0.08;

    // Doty-style brightness detection: for auto/smart mode, decide light vs
    // dark from the image's luminance instead of relying on matugen's
    // internal heuristics, so the choice is deterministic and inspectable.
    let effective_mode = match requested {
        "auto" | "smart" => {
            let (b_code, b_out, _) = run(
                &magick_bin(),
                &[
                    &source,
                    "-colorspace",
                    "Gray",
                    "-format",
                    "%[fx:mean]",
                    "info:",
                ],
            );
            let brightness: f64 = if b_code == 0 {
                b_out.trim().parse().unwrap_or(0.5)
            } else {
                0.5
            };
            if brightness > 0.55 { "light" } else { "dark" }
        }
        other => other,
    };

    let mut cmd = std::process::Command::new(matugen_bin());
    cmd.args([
        "image",
        &source,
        "--mode",
        effective_mode,
        "--source-color-index",
        "0",
        "--json",
        "hex",
    ]);
    if mono {
        cmd.args(["--type", "scheme-monochrome"]);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("ERR matugen could not read {}", source));
    }
    let json_str = String::from_utf8_lossy(&out.stdout).to_string();
    if json_str.trim().is_empty() {
        return Err("ERR matugen wrote no palette".to_string());
    }
    let matugen_json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| e.to_string())?;
    let palette = matugen_to_palette(&matugen_json, requested);
    if !palette_ok_value(&palette) {
        return Err("ERR matugen returned an incomplete palette".to_string());
    }
    let mut palette = palette;
    palette_fix_value(&mut palette).map_err(|e| e.to_string())?;
    if !palette_ok_value(&palette) {
        return Err("ERR the adjusted palette is incomplete".to_string());
    }
    let pretty = serde_json::to_string_pretty(&palette).map_err(|e| e.to_string())?;
    write_atomic(&generated_file(), &format!("{}\n", pretty)).map_err(|e| e.to_string())?;
    let _ = std::fs::copy(generated_file(), &cache);
    Ok(())
}

pub(crate) fn wallpaper_theme(wp: &str, requested: &str) -> String {
    wallpaper_theme_mode(
        wp,
        requested,
        env::var("ANIXIE_WALLPAPER_RECORDED")
            .map(|v| v == "1")
            .unwrap_or(false),
    )
}

/// Like wallpaper_theme, but with the recorded flag as an explicit
/// argument: `wallpaper use` already saved the selection, so it must
/// not record a second global choice on top.
pub(crate) fn wallpaper_theme_recorded(wp: &str, requested: &str, recorded: bool) -> String {
    wallpaper_theme_mode(wp, requested, recorded)
}

fn wallpaper_theme_mode(wp: &str, requested: &str, recorded: bool) -> String {
    // Save an explicit mode for the next picture, like legacy.
    if !requested.is_empty() {
        match requested {
            "auto" | "smart" | "light" | "dark" => {}
            _ => return "ERR mode must be auto, light or dark".to_string(),
        }
        if write_atomic(&wallpaper_mode_file(), &format!("{}\n", requested)).is_err() {
            return "ERR could not write wallpaper-mode".to_string();
        }
    }
    let effective_mode = if requested.is_empty() {
        saved_wallpaper_mode()
    } else {
        requested.to_string()
    };
    let _lock = match StateLock::acquire() {
        Ok(l) => l,
        Err(e) => return format!("ERR could not lock the theme state: {}", e),
    };
    if let Err(e) = generate_palette(wp, &effective_mode) {
        return e;
    }
    if let Err(e) = materialise("wallpaper") {
        return format!("ERR {}", e);
    }
    drop(_lock);
    if !wp.is_empty() && !recorded {
        let _ = crate::wallpaper::select::record_selection(wp, "");
    }
    tell_apps("wallpaper");
    run_hooks("wallpaper");
    "Theme: wallpaper".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matugen_to_palette_maps_and_adds_ansi() {
        let m = serde_json::json!({
            "mode": "dark",
            "colors": {
                "background": {"default": {"color": "#101010"}},
                "surface_container": {"default": {"color": "#222222"}},
                "surface_container_high": {"default": {"color": "#333333"}},
                "surface_container_highest": {"default": {"color": "#444444"}},
                "surface_container_lowest": {"default": {"color": "#000000"}},
                "surface_variant": {"default": {"color": "#555555"}},
                "on_background": {"default": {"color": "#eeeeee"}},
                "on_surface_variant": {"default": {"color": "#dddddd"}},
                "outline": {"default": {"color": "#666666"}},
                "primary": {"default": {"color": "#888888"}},
                "secondary": {"default": {"color": "#999999"}},
                "tertiary": {"default": {"color": "#aaaaaa"}},
                "error": {"default": {"color": "#ff0000"}},
                "on_primary": {"default": {"color": "#000000"}}
            }
        });
        let p = matugen_to_palette(&m, "dark");
        assert_eq!(p["background"], "#101010");
        assert_eq!(p["surface"], "#222222");
        assert_eq!(p["_mode"], "dark");
        assert_eq!(p["ansi0"], "#101010");
        assert_eq!(p["ansi15"], "#eeeeee");
        assert!(super::super::color::palette_ok_value(&p), "got: {p}");
    }
}
