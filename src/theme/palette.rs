use std::env;
use std::path::PathBuf;

pub(crate) fn home_dir() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub(crate) fn presets_dir() -> PathBuf {
    if let Ok(v) = env::var("THEME_PRESETS_DIR") { return PathBuf::from(v); }
    let cfg = crate::config::get_str(&["theme", "presets_dir"], "");
    if !cfg.is_empty() { return PathBuf::from(cfg); }
    home_dir().join(".config/alexie/presets")
}

pub(crate) fn switch_theme(wallpaper: &str, mode: &str) -> String {
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

/// Load a preset palette from an explicit directory (testable).
pub(crate) fn load_preset_palette_from(
    dir: &std::path::Path,
    name: &str,
) -> Option<std::collections::HashMap<String, String>> {
    let path = dir.join(format!("{}.toml", name));
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

pub(crate) fn load_preset_palette(name: &str) -> Option<std::collections::HashMap<String, String>> {
    // Use presets_dir() so THEME_PRESETS_DIR / config.toml are honoured,
    // matching presets(), instead of a hardcoded ~/.config path.
    load_preset_palette_from(&presets_dir(), name)
}

pub(crate) fn get_matugen_palette(path: &std::path::Path, mode: &str) -> Option<std::collections::HashMap<String, String>> {
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_preset_palette_from_explicit_dir() {
        let dir = std::env::temp_dir().join(format!(
            "alexie-preset-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mine.toml"),
            "[colors]\nsurface = \"#112233\"\nprimary = \"#aabbcc\"\n",
        )
        .unwrap();
        let palette = load_preset_palette_from(&dir, "mine").expect("preset loads");
        assert_eq!(palette.get("surface").map(|s| s.as_str()), Some("#112233"));
        assert_eq!(palette.get("primary").map(|s| s.as_str()), Some("#aabbcc"));
        assert!(load_preset_palette_from(&dir, "missing").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
