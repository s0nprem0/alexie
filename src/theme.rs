use crate::proc::{resolve_tool, run};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

const THEMES_JSON: &str = include_str!("../themes.json");

fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}


fn theme_names() -> Vec<String> {
    // parse keys from embedded JSON (simple: lines with quoted keys at top level)
    let mut names = Vec::new();
    // Use a tiny parser: find top-level object keys.
    // themes.json is {"name": {...}, ...}. Extract quoted keys before ':' at depth 1.
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut buf = String::new();
    let mut collecting_key = false;
    for ch in THEMES_JSON.chars() {
        if escape { escape = false; if collecting_key { buf.push(ch); } continue; }
        if ch == '\\' { escape = true; if collecting_key { } ; continue; }
        if in_str {
            if ch == '"' {
                in_str = false;
                if collecting_key && depth == 1 {
                    names.push(buf.clone());
                    buf.clear();
                    collecting_key = false;
                }
            } else {
                if collecting_key { buf.push(ch); }
            }
        } else {
            if ch == '"' {
                in_str = true;
                // a key starts if previous non-space is { or ,
                collecting_key = true;
                buf.clear();
            } else if ch == '{' {
                depth += 1;
                collecting_key = false;
            } else if ch == '}' {
                depth -= 1;
                collecting_key = false;
            } else if ch == ':' && collecting_key {
                // shouldn't happen; reset
                collecting_key = false;
                buf.clear();
            } else if !ch.is_whitespace() && ch != ',' {
                collecting_key = false;
            }
        }
    }
    names
}

fn current_theme() -> String {
    let p = state_dir().join("current");
    if let Ok(s) = std::fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    // fallback: state.json theme field
    let sp = state_dir().join("state.json");
    if let Ok(s) = std::fs::read_to_string(&sp) {
        // naive extract "theme": "name"
        if let Some(i) = s.find("\"theme\"") {
            let rest = &s[i..];
            if let Some(q1) = rest.find('"') {
                let after = &rest[q1+1..];
                // skip to next quote after key
                if let Some(q2) = after.find('"') {
                    let keyend = &after[q2+1..];
                    if let Some(q3) = keyend.find('"') {
                        let val = &keyend[q3+1..];
                        if let Some(q4) = val.find('"') {
                            return val[..q4].to_string();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

fn delegate(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let tool = resolve_tool("theme");
    let (code, out, err) = run(&tool, &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn presets_dir() -> PathBuf {
    if let Ok(v) = env::var("THEME_PRESETS_DIR") { return PathBuf::from(v); }
    let cfg = crate::config::get_str(&["theme", "presets_dir"], "");
    if !cfg.is_empty() { return PathBuf::from(cfg); }
    home_dir().join(".config/alexie/presets")
}

fn switch_theme(wallpaper: &str, mode: &str) -> String {
    if wallpaper.is_empty() {
        return "ERR usage: theme switch WALLPAPER [auto|light|dark]".to_string();
    }
    let path = match std::fs::canonicalize(wallpaper) {
        Ok(p) => p,
        Err(e) => return format!("ERR cannot resolve path: {}", e),
    };
    let home = home_dir();
    let cache_dir = home.join(".cache").join("alexie").join("theme_switcher");
    let hash = {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        path.to_string_lossy().hash(&mut h);
        format!("{:016x}", h.finish())
    };
    let force = mode.to_string();
    let cache_path = cache_dir.join(format!("{}_{}.json", hash, force));
    // cache freshness
    let cache_fresh = match (std::fs::metadata(&cache_path), std::fs::metadata(&path)) {
        (Ok(cm), Ok(wm)) => match (cm.modified(), wm.modified()) {
            (Ok(c), Ok(w)) => c >= w,
            _ => false,
        },
        _ => false,
    };
    if cache_path.exists() && cache_fresh {
        if let Ok(s) = std::fs::read_to_string(&cache_path) {
            return s;
        }
    }
    let mut cmd = std::process::Command::new("matugen");
    cmd.args(["image", path.to_str().unwrap_or(""), "--json", "hex", "--source-color-index", "0"]);
    match mode {
        "light" => { cmd.args(["--mode", "light"]); }
        "dark" => { cmd.args(["--mode", "dark"]); }
        _ => {}
    }
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => return format!("ERR matugen: {}", e),
    };
    if !out.status.success() {
        return format!("ERR matugen: {}", String::from_utf8_lossy(&out.stderr));
    }
    let json = String::from_utf8_lossy(&out.stdout).to_string();
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache_path, &json);
    json
}

fn load_preset_palette(name: &str) -> Option<std::collections::HashMap<String, String>> {
    let path = home_dir().join(".config/alexie/presets").join(format!("{}.toml", name));
    // also try doty wabi presets? no
    let content = std::fs::read_to_string(path).ok()?;
    let value: toml::Value = content.parse().ok()?;
    let colors_table = value.get("colors")?.as_table()?;
    let mut palette = std::collections::HashMap::new();
    for (key, val) in colors_table {
        if let Some(s) = val.as_str() {
            palette.insert(key.clone(), s.to_string());
        }
    }
    Some(palette)
}

fn get_matugen_palette(path: &std::path::Path, mode: &str) -> Option<std::collections::HashMap<String, String>> {
    use std::process::Command;
    let mut cmd = Command::new("matugen");
    cmd.args(["image", path.to_str()?, "--json", "hex", "--source-color-index", "0"]);
    match mode {
        "light" => { cmd.args(["--mode", "light"]); }
        "dark" => { cmd.args(["--mode", "dark"]); }
        _ => {}
    }
    let out = cmd.output().ok()?;
    if !out.status.success() { return None; }
    let json = String::from_utf8_lossy(&out.stdout);
    let data: serde_json::Value = serde_json::from_str(&json).ok()?;
    let colors = data.get("colors")?.as_object()?;
    let mut palette = std::collections::HashMap::new();
    for (name, modev) in colors {
        if let Some(color) = modev.get("default").and_then(|d| d.get("color")).and_then(|c| c.as_str()) {
            palette.insert(name.clone(), color.to_string());
        }
    }
    Some(palette)
}

fn hex_to_rgb_tuple(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        Some((r, g, b))
    } else {
        None
    }
}

fn interpolate_color(c1: &str, c2: &str, factor: f64) -> String {
    let rgb1 = hex_to_rgb_tuple(c1).unwrap_or((0, 0, 0));
    let rgb2 = hex_to_rgb_tuple(c2).unwrap_or((255, 255, 255));
    let r = ((rgb1.0 as f64) * (1.0 - factor) + (rgb2.0 as f64) * factor).round() as u8;
    let g = ((rgb1.1 as f64) * (1.0 - factor) + (rgb2.1 as f64) * factor).round() as u8;
    let b = ((rgb1.2 as f64) * (1.0 - factor) + (rgb2.2 as f64) * factor).round() as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn build_vars(palette: &std::collections::HashMap<String, String>) -> std::collections::HashMap<String, String> {
    let mut vars = std::collections::HashMap::new();
    let bg = palette.get("surface").cloned().unwrap_or_else(|| "#1d2021".to_string());
    let bg_dark = palette.get("surface_container").cloned().unwrap_or_else(|| "#282828".to_string());
    let bg_light = palette.get("surface_variant").cloned().unwrap_or_else(|| "#3c3836".to_string());
    let fg = palette.get("on_surface").cloned().unwrap_or_else(|| "#ebdbb2".to_string());
    let fg_light = palette.get("on_surface_variant").cloned().unwrap_or_else(|| "#d5c4a1".to_string());
    let accent = palette.get("primary").cloned().unwrap_or_else(|| "#a9b665".to_string());
    let secondary = palette.get("secondary").cloned().unwrap_or_else(|| "#7daea3".to_string());
    let tertiary = palette.get("tertiary").cloned().unwrap_or_else(|| "#d8a657".to_string());
    let error = palette.get("error").cloned().unwrap_or_else(|| "#cc241d".to_string());

    let (bg_r, bg_g, bg_b) = hex_to_rgb_tuple(&bg).unwrap_or((0, 0, 0));
    let (fg_r, fg_g, fg_b) = hex_to_rgb_tuple(&fg).unwrap_or((255, 255, 255));
    let bg_brightness = 0.299 * (bg_r as f64) + 0.587 * (bg_g as f64) + 0.114 * (bg_b as f64);
    let fg_brightness = 0.299 * (fg_r as f64) + 0.587 * (fg_g as f64) + 0.114 * (fg_b as f64);
    let is_light = bg_brightness > fg_brightness;

    vars.insert("is_light".to_string(), is_light.to_string());
    vars.insert("bg".to_string(), bg.clone());
    vars.insert("bg_hex".to_string(), bg.trim_start_matches('#').to_string());
    vars.insert("bg_dark".to_string(), bg_dark.clone());
    vars.insert("bg_dark_hex".to_string(), bg_dark.trim_start_matches('#').to_string());
    vars.insert("bg_light".to_string(), bg_light.clone());
    vars.insert("bg_light_hex".to_string(), bg_light.trim_start_matches('#').to_string());
    vars.insert("fg".to_string(), fg.clone());
    vars.insert("fg_hex".to_string(), fg.trim_start_matches('#').to_string());
    vars.insert("fg_light".to_string(), fg_light.clone());
    vars.insert("fg_light_hex".to_string(), fg_light.trim_start_matches('#').to_string());
    vars.insert("accent".to_string(), accent.clone());
    vars.insert("accent_hex".to_string(), accent.trim_start_matches('#').to_string());
    vars.insert("secondary".to_string(), secondary.clone());
    vars.insert("secondary_hex".to_string(), secondary.trim_start_matches('#').to_string());
    vars.insert("tertiary".to_string(), tertiary.clone());
    vars.insert("tertiary_hex".to_string(), tertiary.trim_start_matches('#').to_string());
    vars.insert("error".to_string(), error.clone());
    vars.insert("error_hex".to_string(), error.trim_start_matches('#').to_string());

    let hex_to_rgb = |hex: &str| -> String {
        let h = hex.trim_start_matches('#');
        if h.len() == 6 {
            let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
            format!("{},{},{}", r, g, b)
        } else {
            "0,0,0".to_string()
        }
    };
    vars.insert("bg_rgb".to_string(), hex_to_rgb(&bg));
    vars.insert("bg_dark_rgb".to_string(), hex_to_rgb(&bg_dark));
    vars.insert("bg_light_rgb".to_string(), hex_to_rgb(&bg_light));
    vars.insert("fg_rgb".to_string(), hex_to_rgb(&fg));
    vars.insert("fg_light_rgb".to_string(), hex_to_rgb(&fg_light));
    vars.insert("accent_rgb".to_string(), hex_to_rgb(&accent));
    vars.insert("secondary_rgb".to_string(), hex_to_rgb(&secondary));
    vars.insert("error_rgb".to_string(), hex_to_rgb(&error));

    for i in 0..8 {
        let factor = i as f64 / 7.0;
        let color = interpolate_color(&accent, &tertiary, factor);
        vars.insert(format!("cava_color_{}", i + 1), color);
    }

    vars.insert("home".to_string(), home_dir().to_string_lossy().to_string());
    vars
}

fn vars_cache_path() -> PathBuf {
    home_dir().join(".cache").join("alexie").join("theme_vars.json")
}

fn load_cached_vars() -> Option<std::collections::HashMap<String, String>> {
    let p = vars_cache_path();
    let s = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}

fn save_cached_vars(vars: &std::collections::HashMap<String, String>) {
    let p = vars_cache_path();
    if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(json) = serde_json::to_string(vars) { let _ = std::fs::write(p, json); }
}

fn generate_vars(source: &str, value: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let palette = if source == "preset" {
        // reuse load_preset_palette (defined later? need define)
        load_preset_palette(value).ok_or_else(|| format!("unknown preset {}", value))?
    } else {
        // wallpaper
        let path = std::fs::canonicalize(value).map_err(|e| e.to_string())?;
        get_matugen_palette(&path, "auto").ok_or_else(|| "matugen failed".to_string())?
    };
    Ok(build_vars(&palette))
}

fn theme_vars(args: &[String]) -> String {
    // theme vars [preset NAME | wallpaper PATH | PATH]
    let mut source = "wallpaper".to_string();
    let mut value = String::new();
    if args.len() >= 2 {
        if args[1] == "preset" {
            source = "preset".to_string();
            value = args.get(2).cloned().unwrap_or_default();
        } else if args[1] == "wallpaper" {
            source = "wallpaper".to_string();
            value = args.get(2).cloned().unwrap_or_default();
        } else {
            value = args[1].clone();
            if std::path::Path::new(&value).exists() { source = "wallpaper".to_string(); }
            else { source = "preset".to_string(); }
        }
    }
    if value.is_empty() {
        // try cached
        if let Some(v) = load_cached_vars() {
            return serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string());
        }
        return "ERR usage: theme vars [preset NAME | wallpaper PATH]".to_string();
    }
    match generate_vars(&source, &value) {
        Ok(vars) => {
            save_cached_vars(&vars);
            serde_json::to_string(&vars).unwrap_or_else(|_| "{}".to_string())
        }
        Err(e) => format!("ERR {}", e),
    }
}

fn theme_render(args: &[String]) -> String {
    if args.len() < 3 {
        return "ERR usage: theme render TEMPLATE OUTPUT".to_string();
    }
    let tmpl = &args[1];
    let out = &args[2];
    let vars = load_cached_vars().unwrap_or_default();
    let content = match std::fs::read_to_string(tmpl) {
        Ok(c) => c,
        Err(e) => return format!("ERR read template: {}", e),
    };
    let mut rendered = content;
    for (k, v) in &vars {
        let placeholder = format!("{{{{{}}}}}", k);
        rendered = rendered.replace(&placeholder, v);
    }
    if let Some(parent) = std::path::Path::new(out).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(out, rendered) {
        Ok(_) => format!("rendered {}", out),
        Err(e) => format!("ERR write: {}", e),
    }
}

fn presets() -> String {
    let dir = presets_dir();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    if let Ok(read) = fs::read_dir(&dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") { continue; }
            let content = match fs::read_to_string(&path) { Ok(c) => c, Err(_) => continue };
            let value: toml::Value = match content.parse::<toml::Value>() { Ok(v) => v, Err(_) => continue };
            let colors_table = match value.get("colors").and_then(|v| v.as_table()) { Some(t) => t, None => continue };
            let mut colors: BTreeMap<String, String> = BTreeMap::new();
            for (k, v) in colors_table {
                if let Some(s) = v.as_str() { colors.insert(k.clone(), s.to_string()); }
            }
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            entries.push(serde_json::json!({
                "name": stem,
                "path": path.to_string_lossy(),
                "colors": colors,
            }));
        }
    }
    entries.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        return "ERR theme: missing subcommand".to_string();
    }
    match args[0].as_str() {
        "list" => theme_names().join("\n"),
    "presets" => presets(),
    "switch" => {
        let wp = args.get(1).cloned().unwrap_or_default();
        let mode = args.get(2).cloned().unwrap_or_else(|| "auto".to_string());
        switch_theme(&wp, &mode)
    }
    "vars" => theme_vars(&args[1..]),
    "render" => theme_render(&args[1..]),
        "current" => {
            let c = current_theme();
            if c.is_empty() { "none".to_string() } else { c }
        }
        "next" | "prev" => {
            let names = theme_names();
            if names.is_empty() { return "ERR no themes".to_string(); }
            let cur = current_theme();
            let idx = names.iter().position(|n| n == &cur).unwrap_or(0);
            let newidx = if args[0] == "next" {
                (idx + 1) % names.len()
            } else {
                if idx == 0 { names.len() - 1 } else { idx - 1 }
            };
            let name = &names[newidx];
            delegate(&["set".to_string(), name.clone()])
        }
        "state" => {
            let sp = state_dir().join("state.json");
            if let Ok(s) = std::fs::read_to_string(&sp) { s.trim().to_string() }
            else { "{}".to_string() }
        }
        // heavy: delegate
        _ => delegate(args),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_non_empty_and_contains_known() {
        let names = theme_names();
        assert!(!names.is_empty());
        assert!(names.contains(&"tokyo-night".to_string()));
        assert!(names.contains(&"catppuccin-mocha".to_string()));
        assert_eq!(names.len(), 16);
    }

    #[test]
    fn theme_names_unique() {
        let names = theme_names();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }
}
