//! Native theming engine: materialise, generate, set, apply, auto, doctor.
//!
//! Ports the legacy theme.sh + lib/engine/{core,materialize,apps,generate}.sh
//! logic to Rust. The parts that remain external are the same tools
//! legacy shells out to, now via explicit env-overridable names so
//! tests can fake them on PATH:
//! - `theme-apps` renders the application templates (like @THEME_APPS@)
//! - `matugen`/`magick`/`ffmpeg` for wallpaper palette generation
//! - quickshell/hyprctl/gsettings/tmux/wallpaper-legacy for tell_apps
//!   (all best-effort, like legacy `|| true`)
//!
//! Palettes come from the embedded themes.json (identical copy of
//! anixie's themes.json, verified by `diff` in development), not an
//! external @PALETTES@ path, so no Nix placeholder plumbing is needed
//! for the curated path. The renderer record written into a
//! generation is therefore "embedded themes.json <theme-apps path>"
//! rather than the legacy Nix store paths; `apply` treats an
//! existing renderer file as "made by a compatible renderer".

use super::state::{
    current_mode, current_theme, generated_file, generations_dir, has_theme,
    live_link, mode_file, palettes, read_state_json, saved_wallpaper_mode,
    state_dir, state_ensure, state_set_generation, state_set_theme,
    wallpaper_mode_file, write_atomic,
};
use crate::proc::run;
use std::env;
use std::path::{Path, PathBuf};

fn tool(env_name: &str, default: &str) -> String {
    env::var(env_name).unwrap_or_else(|_| default.to_string())
}

fn theme_apps_bin() -> String {
    // 1. explicit override, 2. the previous generation's renderer
    // record (live/renderer: "<palettes> <theme-apps>", written by
    // legacy materialise with Nix store paths), 3. PATH lookup.
    if let Ok(v) = env::var("THEME_APPS") {
        if !v.is_empty() {
            return v;
        }
    }
    let cfg = crate::config::get_str(&["theme", "theme_apps"], "");
    if !cfg.is_empty() {
        return cfg;
    }
    if let Ok(s) = std::fs::read_to_string(live_link().join("renderer")) {
        if let Some(path) = s.split_whitespace().nth(1) {
            return path.to_string();
        }
    }
    tool("THEME_APPS", "theme-apps")
}
fn matugen_bin() -> String {
    tool("MATUGEN", "matugen")
}
fn magick_bin() -> String {
    tool("MAGICK", "magick")
}
fn ffmpeg_bin() -> String {
    tool("FFMPEG", "ffmpeg")
}
fn quickshell_bin() -> String {
    tool("QUICKSHELL", "quickshell")
}
fn hyprctl_bin() -> String {
    tool("HYPRCTL", "hyprctl")
}
fn gsettings_bin() -> String {
    tool("GSETTINGS", "gsettings")
}
fn tmux_bin() -> String {
    tool("TMUX", "tmux")
}

// ---------- palette validation & colour maths (port of color.sh) ----------

pub(crate) fn color_ok(s: &str) -> bool {
    s.len() == 7
        && s.starts_with('#')
        && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn hex_byte(h: &str, i: usize) -> u8 {
    u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0)
}

fn lin(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub(crate) fn luminance(hex: &str) -> Option<f64> {
    if !color_ok(hex) {
        return None;
    }
    let h = &hex[1..];
    let r = lin(hex_byte(h, 0));
    let g = lin(hex_byte(h, 2));
    let b = lin(hex_byte(h, 4));
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

pub(crate) fn contrast(a: &str, b: &str) -> Option<f64> {
    let mut la = luminance(a)?;
    let mut lb = luminance(b)?;
    if la < lb {
        std::mem::swap(&mut la, &mut lb);
    }
    Some((la + 0.05) / (lb + 0.05))
}

fn mix(a: &str, b: &str, t: f64) -> String {
    let t = t.clamp(0.0, 1.0);
    let ah = &a[1..];
    let bh = &b[1..];
    let mut out = String::from("#");
    for i in (0..6).step_by(2) {
        let av = hex_byte(ah, i) as f64;
        let bv = hex_byte(bh, i) as f64;
        let ch = av + (bv - av) * t;
        out.push_str(&format!("{:02x}", (ch.round() as i64).clamp(0, 255)));
    }
    out
}

/// Port of color_adjust_contrast(): mix fg toward black or white
/// (whichever contrasts more with bg) until the target ratio is
/// reached, via binary search. Returns None if unreachable or
/// inputs are not colours; returns the original if it already passes.
pub(crate) fn adjust_contrast(fg: &str, bg: &str, want: f64) -> Option<String> {
    if !color_ok(fg) || !color_ok(bg) || want <= 0.0 {
        return None;
    }
    if contrast(fg, bg)? >= want {
        return Some(fg.to_string());
    }
    let c_black = contrast("#000000", bg)?;
    let c_white = contrast("#ffffff", bg)?;
    let endpoint = if c_black >= c_white { "#000000" } else { "#ffffff" };
    if contrast(endpoint, bg)? < want {
        return None;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        let cand = mix(fg, endpoint, mid);
        if contrast(&cand, bg)? >= want {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(mix(fg, endpoint, hi))
}

const PALETTE_KEYS: &[&str] = &[
    "background", "surface", "surfaceContainer", "surfaceVariant",
    "foreground", "fgLight", "bgDark", "bgLight", "muted", "outline",
    "primary", "accent", "accentAlt", "secondary", "tertiary",
    "success", "yellow", "danger", "error",
];

pub(crate) fn palette_ok_value(v: &serde_json::Value) -> bool {
    if !v.is_object() {
        return false;
    }
    PALETTE_KEYS.iter().all(|k| {
        v.get(*k)
            .and_then(|x| x.as_str())
            .map(color_ok)
            .unwrap_or(false)
    })
}

pub(crate) fn palette_ok_file(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| palette_ok_value(&v))
        .unwrap_or(false)
}

/// Port of color_palette_fix(): adjust foreground/fgLight/muted/
/// outline against background/surface to 4.5 in place.
pub(crate) fn palette_fix_value(v: &mut serde_json::Value) -> Result<(), String> {
    if !v.is_object() {
        return Err("palette is not an object".to_string());
    }
    for key in ["foreground", "fgLight", "muted", "outline"] {
        let Some(original) = v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string()) else {
            continue;
        };
        if !color_ok(&original) {
            return Err(format!("invalid colour for {key}"));
        }
        let mut current = original.clone();
        for bg_key in ["background", "surface"] {
            let Some(bg) = v.get(bg_key).and_then(|x| x.as_str()).map(|s| s.to_string()) else {
                continue;
            };
            if !color_ok(&bg) {
                return Err(format!("invalid colour for {bg_key}"));
            }
            let fixed = adjust_contrast(&current, &bg, 4.5)
                .ok_or_else(|| format!("could not adjust {key} against {bg_key}"))?;
            current = fixed;
        }
        if current != original {
            if let Some(obj) = v.as_object_mut() {
                obj.insert(key.to_string(), serde_json::json!(current));
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn palette_fix_file(path: &Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut v: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("palette is not an object".to_string());
    }
    palette_fix_value(&mut v)?;
    let pretty = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    write_atomic(&path.to_path_buf(), &format!("{}\n", pretty)).map_err(|e| e.to_string())
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
    let mode = if mode == "light" || mode == "dark" { mode.to_string() } else { "dark".to_string() };
    let (success, yellow) = if mode == "light" {
        ("#2f7d32", "#9a6700")
    } else {
        ("#a6e3a1", "#f0c674")
    };
    let (bg_dark, bg_light) = if mode == "light" {
        (mc(m, "surface_container_highest"), mc(m, "surface_container_lowest"))
    } else {
        (mc(m, "surface_container_lowest"), mc(m, "surface_container_highest"))
    };
    let mut obj = serde_json::Map::new();
    let mut ins = |k: &str, v: String| { obj.insert(k.to_string(), serde_json::json!(v)); };
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

// ---------- kitty conf (port of write_kitty_conf in core.sh) ----------

fn colour(generated: &serde_json::Value, key: &str) -> String {
    generated.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn ansi_colour(generated: &serde_json::Value, index: u8, fallback: &str) -> String {
    let v = generated
        .get(format!("ansi{}", index))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    v.unwrap_or_else(|| colour(generated, fallback))
}

fn write_kitty(generated: &serde_json::Value, dest: &Path) -> Result<(), String> {
    let c = |k: &str| colour(generated, k);
    let a = |i: u8, f: &str| ansi_colour(generated, i, f);
    let content = format!(
        "background {}\nforeground {}\nselection_background {}\nselection_foreground {}\ncolor0 {}\ncolor8 {}\ncolor1 {}\ncolor9 {}\ncolor2 {}\ncolor10 {}\ncolor3 {}\ncolor11 {}\ncolor4 {}\ncolor12 {}\ncolor5 {}\ncolor13 {}\ncolor6 {}\ncolor14 {}\ncolor7 {}\ncolor15 {}\n",
        c("background"),
        c("foreground"),
        c("accent"),
        c("background"),
        a(0, "background"),
        a(8, "muted"),
        a(1, "danger"),
        a(9, "danger"),
        a(2, "success"),
        a(10, "success"),
        a(3, "yellow"),
        a(11, "yellow"),
        a(4, "primary"),
        a(12, "primary"),
        a(5, "accentAlt"),
        a(13, "accentAlt"),
        a(6, "accent"),
        a(14, "accent"),
        a(7, "foreground"),
        a(15, "foreground"),
    );
    write_atomic(&dest.to_path_buf(), &content).map_err(|e| e.to_string())
}

// ---------- state lock (port of state_lock/state_unlock) ----------

pub(crate) struct StateLock {
    file: std::fs::File,
}

impl StateLock {
    fn acquire() -> Result<Self, String> {
        std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
        // Legacy locks $STATE/.lock with flock(2) (a persistent file
        // that stays behind after unlock). Locking the same file
        // with flock(2) here makes Rust and legacy bash callers
        // mutually exclude each other, instead of a create_new lock
        // file that can never be acquired while .lock exists.
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(state_dir().join(".lock"))
            .map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if ret == 0 {
                return Ok(Self { file });
            }
            if std::time::Instant::now() >= deadline {
                return Err("could not lock the theme state (timeout)".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
        // File is left on disk, like legacy flock users expect.
    }
}

// ---------- prune generations (port of prune_generations) ----------

fn prune_generations(current: &Path) {
    let dir = generations_dir();
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut gens: Vec<(std::time::SystemTime, PathBuf)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let m = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((m, p))
        })
        .collect();
    gens.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut keep = 0;
    for (_, gen) in gens {
        if gen == current {
            continue;
        }
        keep += 1;
        if keep <= 2 {
            continue;
        }
        let _ = std::fs::remove_dir_all(&gen);
    }
}

// ---------- materialise (port of materialize.sh) ----------

fn materialise(name: &str) -> Result<(), String> {
    std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(generations_dir()).map_err(|e| e.to_string())?;
    state_ensure()?;

    let mode_value = if name == "wallpaper" {
        if !wallpaper_palette() {
            return Err("the generated palette is missing; run 'theme wallpaper FILE'".to_string());
        }
        std::fs::read_to_string(generated_file())
            .map_err(|e| e.to_string())
            .and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s)
                    .map_err(|e| e.to_string())
            }).map(|v| v.get("_mode").and_then(|m| m.as_str()).unwrap_or("dark").to_string())?
    } else {
        let entry = super::state::palette_entry(name)
            .ok_or_else(|| format!("no such theme: {}", name))?;
        // resolved palette = entry without "_" keys
        let obj = entry.as_object().ok_or_else(|| format!("no such theme: {}", name))?;
        let mut resolved = serde_json::Map::new();
        for (k, v) in obj {
            if !k.starts_with('_') {
                resolved.insert(k.clone(), v.clone());
            }
        }
        let tmp = state_dir().join(format!("palette.{}", std::process::id()));
        let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(resolved))
            .map_err(|e| e.to_string())?;
        write_atomic(&tmp, &format!("{}\n", pretty)).map_err(|e| e.to_string())?;
        if !palette_ok_file(&tmp) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("theme {} is missing colours", name));
        }
        std::fs::rename(&tmp, generated_file()).map_err(|e| e.to_string())?;
        super::state::mode_of(name)
    };

    write_atomic(&mode_file(), &format!("{}\n", mode_value)).map_err(|e| e.to_string())?;

    let gen = {
        let stamp = {
            let (code, out, _) = run("date", &["+%Y%m%d%H%M%S"]);
            if code == 0 { out.trim().to_string() } else { "0".to_string() }
        };
        let mut candidate = generations_dir().join(format!("{}-{}", stamp, std::process::id()));
        let mut n = 0;
        while candidate.exists() {
            n += 1;
            candidate = generations_dir().join(format!("{}-{}-{}", stamp, std::process::id(), n));
        }
        std::fs::create_dir_all(&candidate).map_err(|e| e.to_string())?;
        candidate
    };

    let cleanup = |gen: &Path| { let _ = std::fs::remove_dir_all(gen); };
    if std::fs::copy(generated_file(), gen.join("palette.json")).is_err() {
        cleanup(&gen);
        return Err("could not stage the palette".to_string());
    }
    if write_atomic(&gen.join("mode"), &format!("{}\n", mode_value)).is_err() {
        cleanup(&gen);
        return Err("could not stage the mode".to_string());
    }

    let generated_json: serde_json::Value = std::fs::read_to_string(gen.join("palette.json"))
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))?;
    if write_kitty(&generated_json, &gen.join("kitty.conf")).is_err() {
        cleanup(&gen);
        return Err("could not write the terminal palette".to_string());
    }

    // renderer record: embedded palettes + the theme-apps binary in use
    if write_atomic(
        &gen.join("renderer"),
        &format!("embedded-themes {}\n", theme_apps_bin()),
    )
    .is_err()
    {
        cleanup(&gen);
        return Err("could not record the renderer".to_string());
    }

    // theme-apps renders the application files; it is the one
    // external renderer legacy used (@THEME_APPS@).
    let (code, _, err) = run(&theme_apps_bin(), &[name, gen.to_string_lossy().as_ref()]);
    if code != 0 {
        cleanup(&gen);
        return Err(format!("could not render the application themes: {}", err.trim()));
    }

    for source in [
        "palette.json", "mode", "kitty.conf", "helix.toml", "yazi.toml", "hyprlock.conf",
        "qt6ct-colors.conf", "qt6ct.conf", "qt5ct.conf", "kvantum.kvconfig",
        "gtk-3.0-settings.ini", "gtk-4.0-settings.ini", "gtk-3.0.css", "gtk-4.0.css",
        "share-picker.css", "discord.css", "librewolf-userChrome.css", "librewolf-userContent.css",
        "tmux.conf", "btop.theme", "terminal.zsh",
    ] {
        if std::fs::metadata(gen.join(source)).map(|m| m.len()).unwrap_or(0) == 0 {
            cleanup(&gen);
            return Err(format!("the generation is missing {}", source));
        }
    }

    // Activate: tmp symlink + rename (atomic, like legacy .live.$$
    let tmp_link = state_dir().join(format!(".live.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp_link);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        if symlink(&gen, &tmp_link).is_err() {
            cleanup(&gen);
            return Err("could not activate the generation".to_string());
        }
    }
    #[cfg(not(unix))]
    {
        cleanup(&gen);
        return Err("could not activate the generation".to_string());
    }
    if std::fs::rename(&tmp_link, live_link()).is_err() {
        let _ = std::fs::remove_file(&tmp_link);
        cleanup(&gen);
        return Err("could not activate the generation".to_string());
    }

    let source = if name == "wallpaper" { "wallpaper" } else { "preset" };
    write_atomic(&super::state::current_file(), &format!("{}\n", name)).map_err(|e| e.to_string())?;
    state_set_theme(name, source)?;
    let gen_name = gen.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if state_set_generation(&gen_name).is_err() {
        eprintln!("theme: could not record the new generation");
    }

    prune_generations(&gen);
    Ok(())
}

fn wallpaper_palette() -> bool {
    generated_file().exists()
        && palette_ok_file(&generated_file())
        && std::fs::read_to_string(generated_file())
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| v.get("_mode").is_some())
            .unwrap_or(false)
}

// ---------- tell_apps / hooks (port of apps.sh) ----------

pub(crate) fn color_accent_name(hex: &str) -> Option<String> {
    if !color_ok(hex) {
        return None;
    }
    let h = &hex[1..];
    let r = hex_byte(h, 0) as f64 / 255.0;
    let g = hex_byte(h, 2) as f64 / 255.0;
    let b = hex_byte(h, 4) as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d < 0.08 || max < 0.1 || min > 0.9 {
        return Some("slate".to_string());
    }
    let hue = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let hue = if hue < 0.0 { hue + 360.0 } else { hue };
    // legacy maps hue ranges to GTK accent names; port below
    let name = match hue {
        h if h < 15.0 || h >= 345.0 => "red",
        h if h < 45.0 => "orange",
        h if h < 70.0 => "yellow",
        h if h < 165.0 => "green",
        h if h < 195.0 => "teal",
        h if h < 250.0 => "blue",
        h if h < 300.0 => "purple",
        _ => "pink",
    };
    Some(name.to_string())
}

fn generated_palette() -> serde_json::Value {
    std::fs::read_to_string(generated_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn tell_apps(name: &str) {
    let palette_path = live_link().join("palette.json");
    let qs = quickshell_bin();
    if name == "wallpaper" {
        let content = std::fs::read_to_string(&palette_path)
            .or_else(|_| std::fs::read_to_string(generated_file()))
            .unwrap_or_default()
            .replace('\n', "");
        let _ = run(&qs, &["ipc", "call", "theme", "palette", &content]);
    } else {
        let _ = run(&qs, &["ipc", "call", "theme", "set", name]);
    }
    let _ = run(&hyprctl_bin(), &["reload"]);
    let _ = run("pkill", &["-USR1", "-x", "kitty"]);

    let mode = std::fs::read_to_string(live_link().join("mode"))
        .or_else(|_| std::fs::read_to_string(mode_file()))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let (gtk_theme, icon_theme, color_scheme) = if mode == "light" {
        ("adw-gtk3", "Papirus-Light", "prefer-light")
    } else {
        ("adw-gtk3-dark", "Papirus-Dark", "prefer-dark")
    };
    let gs = gsettings_bin();
    let _ = run(&gs, &["set", "org.gnome.desktop.interface", "color-scheme", color_scheme]);
    let _ = run(&gs, &["set", "org.g gnome.desktop.interface", "gtk-theme", gtk_theme]);
    let _ = run(&gs, &["set", "org.gnome.desktop.interface", "gtk-theme", gtk_theme]);
    let _ = run(&gs, &["set", "org.gnome.desktop.interface", "icon-theme", icon_theme]);
    let generated = generated_palette();
    let primary = colour(&generated, "primary");
    if let Some(accent) = color_accent_name(&primary) {
        let _ = run(&gs, &["set", "org.gnome.desktop.interface", "accent-color", &accent]);
    }

    // Equicord, best-effort like legacy
    let config_home = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", super::palette::home_dir().display()));
    let equicord = PathBuf::from(&config_home).join("Equicord");
    let live_discord = live_link().join("discord.css");
    if equicord.is_dir() && live_discord.exists() {
        let _ = std::fs::create_dir_all(equicord.join("themes"));
        let _ = std::fs::copy(&live_discord, equicord.join("themes").join("anixie.theme.css"));
        let settings = equicord.join("settings").join("settings.json");
        if settings.exists() {
            if let Ok(s) = std::fs::read_to_string(&settings) {
                if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(obj) = v.as_object_mut() {
                        let mut themes: Vec<serde_json::Value> = obj
                            .get("enabledThemes")
                            .and_then(|t| t.as_array())
                            .cloned()
                            .unwrap_or_default();
                        if !themes.contains(&serde_json::json!("anixie")) {
                            themes.push(serde_json::json!("anixie"));
                        }
                        obj.insert("enabledThemes".into(), serde_json::Value::Array(themes));
                    }
                    let _ = std::fs::write(&settings, serde_json::to_string(&v).unwrap_or_default());
                }
            }
        }
    }

    super::apply_cursor();
    let _ = run(&tmux_bin(), &["source-file", live_link().join("tmux.conf").to_string_lossy().as_ref()]);
    // wallpaper apply via the legacy binary (best-effort)
    let _ = run("wallpaper-legacy", &["apply"]);
}

fn run_hooks(name: &str) {
    let config_home = env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", super::palette::home_dir().display()));
    let hook_dir = PathBuf::from(&config_home).join("anixie").join("theme-hooks");
    if !hook_dir.is_dir() {
        return;
    }
    let mode = std::fs::read_to_string(live_link().join("mode"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "dark".to_string());
    let generated = generated_palette();
    let c = |k: &str| colour(&generated, k);
    let entries = match std::fs::read_dir(&hook_dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sh") {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&path).map(|m| m.permissions().mode() & 0o111 == 0).unwrap_or(true) {
                continue;
            }
        }
        let envs: Vec<(&str, String)> = vec![
            ("ANIXIE_THEME", name.to_string()),
            ("ANIXIE_MODE", mode.clone()),
            ("ANIXIE_BG", c("background")),
            ("ANIXIE_FG", c("foreground")),
            ("ANIXIE_SURFACE", c("surface")),
            ("ANIXIE_MUTED", c("muted")),
            ("ANIXIE_PRIMARY", c("primary")),
            ("ANIXIE_ACCENT", c("accent")),
            ("ANIXIE_ACCENT_ALT", c("accentAlt")),
            ("ANIXIE_SECONDARY", c("secondary")),
            ("ANIXIE_TERTIARY", c("tertiary")),
            ("ANIXIE_YELLOW", c("yellow")),
            ("ANIXIE_DANGER", c("danger")),
            ("ANIXIE_ERROR", c("error")),
            ("ANIXIE_SUCCESS", c("success")),
            ("ANIXIE_OUTLINE", c("outline")),
            ("ANIXIE_SURFACE_CONTAINER", c("surfaceContainer")),
            ("ANIXIE_SURFACE_VARIANT", c("surfaceVariant")),
            ("ANIXIE_BG_DARK", c("bgDark")),
            ("ANIXIE_BG_LIGHT", c("bgLight")),
            ("ANIXIE_FG_LIGHT", c("fgLight")),
        ];
        let _ = std::process::Command::new(&path)
            .envs(envs)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

// ---------- set / apply (port of set_theme/apply) ----------

pub(crate) fn set_theme(name: &str) -> String {
    if !has_theme(name) {
        return format!("ERR no theme called \"{}\" (try: theme list)", name);
    }
    let _lock = match StateLock::acquire() {
        Ok(l) => l,
        Err(e) => return format!("ERR could not lock the theme state: {}", e),
    };
    if let Err(e) = materialise(name) {
        return format!("ERR {}", e);
    }
    drop(_lock);
    tell_apps(name);
    run_hooks(name);
    format!("Theme: {}", name)
}

pub(crate) fn apply_theme() -> String {
    let _lock = match StateLock::acquire() {
        Ok(l) => l,
        Err(e) => return format!("ERR could not lock the theme state: {}", e),
    };
    let name = {
        let cur = current_theme();
        if has_theme(&cur) || cur == "wallpaper" { cur } else { "tokyo-night".to_string() }
    };
    let state_theme = read_state_json()
        .and_then(|v| v.get("theme").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    let renderer = std::fs::read_to_string(live_link().join("renderer"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let needs_materialise = !live_link().join("palette.json").exists()
        || state_theme != name
        || renderer.is_empty();
    if needs_materialise {
        if let Err(e) = materialise(&name) {
            return format!("ERR {}", e);
        }
    }
    drop(_lock);
    tell_apps(&name);
    run_hooks(&name);
    "OK".to_string()
}

// ---------- generate / wallpaper (port of generate/wallpaper_theme) ----------

fn matugen_version() -> String {
    let (code, out, _) = run(&matugen_bin(), &["--version"]);
    if code == 0 { out.lines().next().unwrap_or("").to_string() } else { String::new() }
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
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(&wp_path), std::fs::canonicalize(&wallpaper_png)) {
        if a == b {
            return Err("ERR the current background is generated from the palette; set a picture with 'wallpaper set FILE' first".to_string());
        }
    }

    let mtime = file_mtime(Path::new(&wp_path));
    let key_material = format!("{}\n{}\n{}\n{}\n{}", wp_path, mtime, requested, "1", matugen_version());
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
            if let Ok(m) = entry.metadata() {
                if let Ok(modified) = m.modified() {
                    if modified.elapsed().map(|d| d.as_secs() > 30 * 86400).unwrap_or(false) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
    if cache.exists() && palette_ok_file(&cache) {
        let ok_mode = std::fs::read_to_string(&cache)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| v.get("_mode").and_then(|m| m.as_str()).map(|s| s == "dark" || s == "light").unwrap_or(false))
            .unwrap_or(false);
        if ok_mode {
            std::fs::copy(&cache, generated_file()).map_err(|e| e.to_string())?;
            return Ok(());
        }
        let _ = std::fs::remove_file(&cache);
    }

    let ext = Path::new(&wp_path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mut source = wp_path.clone();
    if matches!(ext.as_str(), "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "ogv") {
        let frame = cache_dir.join(format!("frame-{}.jpg", key));
        if !frame.exists() {
            let (code, _, err) = run(&ffmpeg_bin(), &["-y", "-loglevel", "error", "-i", &wp_path, "-frames:v", "1", "-q:v", "2", frame.to_string_lossy().as_ref()]);
            if code != 0 {
                return Err(format!("ERR could not read a frame from {}: {}", wp_path, err.trim()));
            }
        }
        source = frame.to_string_lossy().to_string();
    }

    let (sat_code, sat_out, _) = run(&magick_bin(), &[&source, "-colorspace", "HSL", "-channel", "G", "-separate", "-format", "%[fx:mean]", "info:"]);
    let saturation: f64 = if sat_code == 0 { sat_out.trim().parse().unwrap_or(1.0) } else { 1.0 };
    let mono = saturation < 0.08;

    let mut cmd = std::process::Command::new(matugen_bin());
    cmd.args(["image", &source, "--mode", requested, "--source-color-index", "0", "--json", "hex"]);
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
    let matugen_json: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| e.to_string())?;
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
    if !wp.is_empty() && !env::var("ANIXIE_WALLPAPER_RECORDED").map(|v| v == "1").unwrap_or(false) {
        let _ = run("wallpaper-legacy", &["record", wp]);
    }
    tell_apps("wallpaper");
    run_hooks("wallpaper");
    "Theme: wallpaper".to_string()
}

// ---------- auto (port of auto_theme) ----------

fn auto_minutes(s: &str) -> i64 {
    let mut parts = s.split(':');
    let hour: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let minute: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    hour * 60 + minute
}

pub(crate) fn auto_mode(light: &str, dark: &str) -> String {
    auto_mode_at(light, dark, &{
        let (code, out, _) = run("date", &["+%H:%M"]);
        if code == 0 { out.trim().to_string() } else { "12:00".to_string() }
    })
}

fn auto_mode_at(light: &str, dark: &str, now: &str) -> String {
    let light_at = auto_minutes(light);
    let dark_at = auto_minutes(dark);
    let now_at = auto_minutes(now);
    if light_at <= dark_at {
        if now_at >= light_at && now_at < dark_at { "light".into() } else { "dark".into() }
    } else if now_at >= light_at || now_at < dark_at {
        "light".into()
    } else {
        "dark".into()
    }
}

/// Find the paired/family/mode theme for auto switching, in order:
/// explicit _pair, then same family + target mode, then any target
/// mode theme (legacy falls back with a stderr note).
pub(crate) fn find_auto_pair(palettes: &serde_json::Value, current: &str, target_mode: &str) -> Option<String> {
    let obj = palettes.as_object()?;
    let cur_entry = obj.get(current)?;
    let pair = cur_entry.get("_pair").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    if let Some(p) = pair {
        if obj.contains_key(p) {
            return Some(p.to_string());
        }
    }
    let family = cur_entry.get("_family").and_then(|v| v.as_str()).unwrap_or("");
    // same family + target mode, sorted for determinism
    let mut candidates: Vec<&String> = obj.keys().filter(|k| *k != current).collect();
    candidates.sort();
    for k in &candidates {
        let e = &obj[*k];
        if e.get("_mode").and_then(|v| v.as_str()) == Some(target_mode)
            && e.get("_family").and_then(|v| v.as_str()).unwrap_or("") == family
        {
            return Some((*k).clone());
        }
    }
    for k in &candidates {
        let e = &obj[*k];
        if e.get("_mode").and_then(|v| v.as_str()) == Some(target_mode) {
            return Some((*k).clone());
        }
    }
    None
}

fn config_auto(light_key: &str, dark_key: &str) -> (String, String, bool) {
    let light = env::var("AUTO_LIGHT")
        .unwrap_or_else(|_| crate::config::get_str(&["theme", "auto", "light"], "07:00"));
    let dark = env::var("AUTO_DARK")
        .unwrap_or_else(|_| crate::config::get_str(&["theme", "auto", "dark"], "19:00"));
    let enabled = env::var("AUTO_SCHEDULE")
        .map(|v| v == "true")
        .unwrap_or_else(|_| crate::config::get_bool(&["theme", "auto", "schedule", "enabled"], false));
    let _ = (light_key, dark_key);
    (light, dark, enabled)
}

pub(crate) fn apply_auto() -> String {
    let _ = state_ensure();
    let (light, dark, enabled) = config_auto("light", "dark");
    if !enabled {
        return "OK".to_string();
    }
    let state = read_state_json();
    if state.as_ref().and_then(|s| s.get("pinned")).and_then(|p| p.as_bool()).unwrap_or(false) {
        return "OK".to_string();
    }
    let target = auto_mode(&light, &dark);
    if current_mode() == target {
        return "OK".to_string();
    }
    let name = current_theme();
    if name == "wallpaper" {
        // regenerate from the wallpaper fallback path, like legacy
        let source = std::fs::read_to_string(state_dir().join("wallpapers.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("themes")
                    .and_then(|t| t.get("wallpaper"))
                    .and_then(|t| t.get("fallback"))
                    .and_then(|f| f.get("path"))
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        if !source.is_empty() && Path::new(&source).is_file() {
            return wallpaper_theme(&source, &target);
        }
        return "OK".to_string();
    }
    if let Some(pair) = find_auto_pair(&palettes(), &name, &target) {
        if has_theme(&pair) {
            return set_theme(&pair);
        }
    }
    "OK".to_string()
}

// ---------- doctor (port of doctor_theme) ----------

pub(crate) fn doctor() -> String {
    let _ = state_ensure();
    let mut failures: i64 = 0;
    let mut warnings: i64 = 0;
    let mut out = String::new();

    let live = live_link();
    let live_target = std::fs::read_link(&live).ok();
    if live_target.is_none() || !live.join("palette.json").exists() {
        out.push_str("missing  live generation\n");
        failures += 1;
    } else {
        let gen_name = live_target
            .as_ref()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_default();
        out.push_str(&format!("ok       live generation {}\n", gen_name));
        if !palette_ok_file(&live.join("palette.json")) {
            out.push_str("invalid  live palette\n");
            failures += 1;
        } else {
            out.push_str("ok       palette\n");
        }
        for file in [
            "kitty.conf", "helix.toml", "yazi.toml", "hyprlock.conf",
            "qt6ct-colors.conf", "qt6ct.conf", "qt5ct.conf", "kvantum.kvconfig",
            "gtk-3.0-settings.ini", "gtk-4.0-settings.ini", "gtk-3.0.css", "gtk-4.0.css",
            "share-picker.css", "discord.css", "librewolf-userChrome.css", "librewolf-userContent.css",
            "tmux.conf", "btop.theme", "terminal.zsh",
        ] {
            if std::fs::metadata(live.join(file)).map(|m| m.len()).unwrap_or(0) == 0 {
                out.push_str(&format!("missing  {}\n", file));
                failures += 1;
            }
        }
        // unresolved @TOKEN@ placeholders anywhere under live/
        let mut unresolved = false;
        if let Ok(read) = std::fs::read_dir(&live) {
            for entry in read.flatten() {
                if let Ok(s) = std::fs::read_to_string(entry.path()) {
                    if s.lines().any(|line| {
                        let mut chars = line.chars().peekable();
                        // crude @TOKEN@ scan without regex: @, then [A-Z0-9_]+, then @
                        let mut in_at = false;
                        let mut tok = String::new();
                        for ch in chars.by_ref() {
                            if ch == '@' {
                                if in_at && tok.len() >= 2 && tok.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
                                    return true;
                                }
                                in_at = !in_at;
                                tok.clear();
                            } else if in_at {
                                tok.push(ch);
                            }
                        }
                        false
                    }) {
                        unresolved = true;
                        break;
                    }
                }
            }
        }
        if unresolved {
            out.push_str("invalid  unresolved placeholders in the live generation\n");
            failures += 1;
        }

        if live.join("palette.json").exists() {
            if let Ok(s) = std::fs::read_to_string(live.join("palette.json")) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    for (fg_tok, bg_tok) in [
                        ("muted", "surface"),
                        ("muted", "background"),
                        ("fgLight", "surface"),
                        ("fgLight", "background"),
                    ] {
                        let fg_c = v.get(fg_tok).and_then(|x| x.as_str()).unwrap_or("");
                        let bg_c = v.get(bg_tok).and_then(|x| x.as_str()).unwrap_or("");
                        if let (Some(fg_c), Some(bg_c)) = (Some(fg_c), Some(bg_c)) {
                            if let Some(ratio) = contrast(fg_c, bg_c) {
                                if ratio < 4.5 {
                                    out.push_str(&format!("warn     {} vs {} contrast {:.2} < 4.5\n", fg_tok, bg_tok, ratio));
                                    warnings += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let config_home = env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", super::palette::home_dir().display()));
    for cfg in [
        "helix/themes/anixie.toml",
        "yazi/theme.toml",
        "hypr/hyprlock.conf",
        "btop/themes/anixie.theme",
    ] {
        let full = PathBuf::from(&config_home).join(cfg);
        if full.exists() && !full.is_symlink() {
            out.push_str(&format!("warn     {} is not a symlink\n", full.display()));
            warnings += 1;
        }
    }

    for (name, bin) in [("quickshell", quickshell_bin()), ("hyprctl", hyprctl_bin())] {
        if crate::proc::find_in_path(&bin, &env::var("PATH").unwrap_or_default()).is_none()
            && which(&bin).is_none()
        {
            out.push_str(&format!("warn     {} not found in PATH\n", name));
            warnings += 1;
        }
    }

    let wp = state_dir().join("wallpapers.json");
    if wp.exists() {
        let valid = std::fs::read_to_string(&wp)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some();
        if valid {
            out.push_str("ok       wallpaper state\n");
        } else {
            out.push_str("invalid  wallpaper state\n");
            failures += 1;
        }
    }

    if failures != 0 {
        out.push_str(&format!("{} problem(s), {} warning(s)\n", failures, warnings));
        out.trim_end().to_string()
    } else if warnings != 0 {
        out.push_str(&format!("no problems found, {} warning(s)\n", warnings));
        out.trim_end().to_string()
    } else {
        out.push_str("no problems found\n");
        out.trim_end().to_string()
    }
}

fn which(name: &str) -> Option<PathBuf> {
    crate::proc::find_in_path(name, &env::var("PATH").unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_ok_requires_core_colours() {
        let mut v = serde_json::json!({
            "background": "#000000", "surface": "#111111", "surfaceContainer": "#222222",
            "surfaceVariant": "#333333", "foreground": "#eeeeee", "fgLight": "#dddddd",
            "bgDark": "#000000", "bgLight": "#222222", "muted": "#aaaaaa", "outline": "#555555",
            "primary": "#888888", "accent": "#999999", "accentAlt": "#888888", "secondary": "#777777",
            "tertiary": "#666666", "success": "#00ff00", "yellow": "#ffff00", "danger": "#ff0000", "error": "#ff0000"
        });
        assert!(palette_ok_value(&v));
        v["background"] = serde_json::json!("null");
        assert!(!palette_ok_value(&v));
        v.as_object_mut().unwrap().remove("primary");
        assert!(!palette_ok_value(&v));
    }

    #[test]
    fn contrast_and_adjust() {
        assert!(contrast("#000000", "#ffffff").unwrap() > 20.0);
        assert!(contrast("#777777", "#777777").unwrap() < 2.0);
        // grey on same grey must be adjusted toward black or white to pass 4.5
        let fixed = adjust_contrast("#777777", "#777777", 4.5).expect("adjustable");
        assert!(contrast(&fixed, "#777777").unwrap() >= 4.5 - 1e-9, "fixed={fixed}");
        // already passing returns original
        assert_eq!(adjust_contrast("#000000", "#ffffff", 4.5), Some("#000000".to_string()));
        assert_eq!(adjust_contrast("not-a-colour", "#ffffff", 4.5), None);
    }

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
        assert!(palette_ok_value(&p), "got: {p}");
    }

    #[test]
    fn auto_mode_schedule() {
        assert_eq!(auto_mode_at("07:00", "19:00", "12:00"), "light");
        assert_eq!(auto_mode_at("07:00", "19:00", "22:00"), "dark");
        assert_eq!(auto_mode_at("07:00", "19:00", "06:00"), "dark");
        // overnight schedule (light starts at night)
        assert_eq!(auto_mode_at("19:00", "07:00", "22:00"), "light");
        assert_eq!(auto_mode_at("19:00", "07:00", "12:00"), "dark");
    }

    #[test]
    fn find_auto_pair_prefers_pair_then_family() {
        let pals = serde_json::json!({
            "cur-dark": {"_mode": "dark", "_family": "fam", "_pair": "paired-light"},
            "paired-light": {"_mode": "light", "_family": "fam"},
            "other-light": {"_mode": "light", "_family": "other"},
            "other-dark": {"_mode": "dark", "_family": "other"}
        });
        assert_eq!(find_auto_pair(&pals, "cur-dark", "light"), Some("paired-light".to_string()));
        let pals2 = serde_json::json!({
            "cur-dark": {"_mode": "dark", "_family": "fam"},
            "fam-light": {"_mode": "light", "_family": "fam"},
            "other-light": {"_mode": "light", "_family": "other"}
        });
        assert_eq!(find_auto_pair(&pals2, "cur-dark", "light"), Some("fam-light".to_string()));
    }
}
