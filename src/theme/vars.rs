use super::palette::{get_matugen_palette, home_dir, load_preset_palette, presets_dir};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub(crate) fn hex_to_rgb_tuple(hex: &str) -> Option<(u8, u8, u8)> {
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

pub(crate) fn interpolate_color(c1: &str, c2: &str, factor: f64) -> String {
    let rgb1 = hex_to_rgb_tuple(c1).unwrap_or((0, 0, 0));
    let rgb2 = hex_to_rgb_tuple(c2).unwrap_or((255, 255, 255));
    let r = ((rgb1.0 as f64) * (1.0 - factor) + (rgb2.0 as f64) * factor).round() as u8;
    let g = ((rgb1.1 as f64) * (1.0 - factor) + (rgb2.1 as f64) * factor).round() as u8;
    let b = ((rgb1.2 as f64) * (1.0 - factor) + (rgb2.2 as f64) * factor).round() as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

pub(crate) fn build_vars(palette: &std::collections::HashMap<String, String>) -> std::collections::HashMap<String, String> {
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

pub(crate) fn vars_cache_path() -> PathBuf {
    home_dir().join(".cache").join("alexie").join("theme_vars.json")
}

pub(crate) fn load_cached_vars() -> Option<std::collections::HashMap<String, String>> {
    let p = vars_cache_path();
    let s = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}

pub(crate) fn save_cached_vars(vars: &std::collections::HashMap<String, String>) {
    let p = vars_cache_path();
    if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(json) = serde_json::to_string(vars) { let _ = std::fs::write(p, json); }
}

pub(crate) fn generate_vars(source: &str, value: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let palette = if source == "preset" {
        load_preset_palette(value).ok_or_else(|| format!("unknown preset {}", value))?
    } else {
        // wallpaper
        let path = std::fs::canonicalize(value).map_err(|e| e.to_string())?;
        get_matugen_palette(&path, "auto").ok_or_else(|| "matugen failed".to_string())?
    };
    Ok(build_vars(&palette))
}

/// Parse the arguments after `theme vars`.
///
/// `args` is the slice *after* the `vars` token, so:
/// - `[]` → no explicit source/value (use cache)
/// - `["preset", NAME]` / `["wallpaper", PATH]` → explicit
/// - `[VALUE]` → inferred: existing path = wallpaper, otherwise preset
///
/// Returns `Ok(None)` for the no-args case, `Ok(Some((source, value)))`
/// for an explicit/inferred choice, and `Err(usage)` for a malformed
/// invocation. The previous implementation indexed `args[1]`/`args[2]`
/// (as if `vars` itself were still in the slice), so explicit
/// `vars preset NAME` / `vars wallpaper PATH` were parsed by accident
/// via the path-exists fallback and `vars preset` with no name fell
/// through to the cache path instead of a usage error.
pub(crate) fn parse_vars_args(args: &[String]) -> Result<Option<(String, String)>, String> {
    const USAGE: &str = "ERR usage: theme vars [preset NAME | wallpaper PATH]";
    match args {
        [] => Ok(None),
        [kind, value] if kind == "preset" || kind == "wallpaper" => {
            Ok(Some((kind.clone(), value.clone())))
        }
        [kind] if kind == "preset" || kind == "wallpaper" => Err(USAGE.to_string()),
        [value] => {
            let source = if std::path::Path::new(value).exists() {
                "wallpaper"
            } else {
                "preset"
            };
            Ok(Some((source.to_string(), value.clone())))
        }
        _ => Err(USAGE.to_string()),
    }
}

pub(crate) fn theme_vars(args: &[String]) -> String {
    // theme vars [preset NAME | wallpaper PATH | PATH]
    let parsed = match parse_vars_args(args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some((source, value)) = parsed else {
        // No explicit choice: return cached vars if we have them.
        if let Some(v) = load_cached_vars() {
            return serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string());
        }
        return "ERR usage: theme vars [preset NAME | wallpaper PATH]".to_string();
    };
    if value.is_empty() {
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

/// Pure template substitution, extracted so tests do not need the
/// vars cache on disk.
pub(crate) fn render_template(content: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let mut rendered = content.to_string();
    for (k, v) in vars {
        let placeholder = format!("{{{{{}}}}}", k);
        rendered = rendered.replace(&placeholder, v);
    }
    rendered
}

pub(crate) fn theme_render(args: &[String]) -> String {
    // `args` is the slice after `render`, so exactly [TEMPLATE, OUTPUT].
    // The old check (`args.len() < 3` with `args[1]`/`args[2]`) made
    // `theme render T O` — a 2-element slice — always hit the usage error.
    if args.len() != 2 {
        return "ERR usage: theme render TEMPLATE OUTPUT".to_string();
    }
    let tmpl = &args[0];
    let out = &args[1];
    let vars = load_cached_vars().unwrap_or_default();
    let content = match std::fs::read_to_string(tmpl) {
        Ok(c) => c,
        Err(e) => return format!("ERR read template: {}", e),
    };
    let rendered = render_template(&content, &vars);
    if let Some(parent) = std::path::Path::new(out).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    match std::fs::write(out, rendered) {
        Ok(_) => format!("rendered {}", out),
        Err(e) => format!("ERR write: {}", e),
    }
}

pub(crate) fn presets() -> String {
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


#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn parse_vars_args_explicit() {
        assert_eq!(parse_vars_args(&[]), Ok(None));
        assert_eq!(
            parse_vars_args(&s(&["preset", "gruvbox-dark"])),
            Ok(Some(("preset".to_string(), "gruvbox-dark".to_string())))
        );
        assert_eq!(
            parse_vars_args(&s(&["wallpaper", "/tmp/wp.png"])),
            Ok(Some(("wallpaper".to_string(), "/tmp/wp.png".to_string())))
        );
    }

    #[test]
    fn parse_vars_args_inferred_single() {
        assert_eq!(
            parse_vars_args(&s(&["gruvbox-dark"])),
            Ok(Some(("preset".to_string(), "gruvbox-dark".to_string())))
        );
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        assert_eq!(
            parse_vars_args(&s(&[&dir])),
            Ok(Some(("wallpaper".to_string(), dir.clone())))
        );
    }

    #[test]
    fn parse_vars_args_rejects_malformed() {
        for bad in [
            s(&["preset"]),
            s(&["wallpaper"]),
            s(&["preset", "a", "extra"]),
            s(&["a", "b", "c"]),
        ] {
            let err = parse_vars_args(&bad).expect_err("malformed vars args");
            assert!(err.starts_with("ERR usage: theme vars"), "got: {err} for {bad:?}");
        }
    }

    #[test]
    fn theme_vars_preset_missing_name_is_usage() {
        let out = theme_vars(&s(&["preset"]));
        assert!(out.starts_with("ERR usage: theme vars"), "got: {out}");
        let out = theme_vars(&s(&["wallpaper"]));
        assert!(out.starts_with("ERR usage: theme vars"), "got: {out}");
        let out = theme_vars(&s(&["a", "b", "c"]));
        assert!(out.starts_with("ERR usage: theme vars"), "got: {out}");
    }

    #[test]
    fn theme_vars_unknown_preset() {
        let out = theme_vars(&s(&["preset", "no-such-preset-xyz"]));
        assert!(out.starts_with("ERR unknown preset"), "got: {out}");
    }

    #[test]
    fn render_template_substitutes() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("bg".to_string(), "#112233".to_string());
        vars.insert("fg".to_string(), "#ddeeff".to_string());
        let out = render_template("bg={{bg}} fg={{fg}} other={{missing}}", &vars);
        assert_eq!(out, "bg=#112233 fg=#ddeeff other={{missing}}");
    }

    #[test]
    fn theme_render_usage() {
        for bad in [s(&["render"]), s(&["render", "only"]), s(&["render", "a", "b", "c"])] {
            // theme_render is called with args after `render`
            let args_after_render = &bad[1..];
            let out = theme_render(args_after_render);
            assert!(out.starts_with("ERR usage: theme render"), "got: {out} for {bad:?}");
        }
    }

    #[test]
    fn theme_render_writes_output() {
        let dir = std::env::temp_dir().join(format!(
            "alexie-render-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tmpl = dir.join("tmpl.txt");
        let out = dir.join("sub").join("out.txt");
        std::fs::write(&tmpl, "hello {{bg}}\n").unwrap();
        let res = theme_render(&[
            tmpl.to_string_lossy().to_string(),
            out.to_string_lossy().to_string(),
        ]);
        assert!(res.starts_with("rendered "), "got: {res}");
        assert!(out.is_file(), "output file created");
        let body = std::fs::read_to_string(&out).unwrap();
        assert!(body.starts_with("hello "), "got: {body}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
