use std::env;
use std::fs;
use std::path::PathBuf;

const THEMES_JSON: &str = include_str!("../../themes.json");

pub(crate) fn state_dir() -> PathBuf {
    // Legacy theme state lives in $XDG_STATE_HOME/anixie/theme
    // (anixie.theme.stateDir), not directly in .../anixie. Reading
    // .../anixie/current always missed, so `theme current` returned
    // "none" even with a theme set.
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie").join("theme")
}

/// Theme names from the embedded palette file, sorted.
///
/// Uses serde_json rather than a hand-rolled parser: the old parser
/// mishandled `\\` escapes and brace depth inside strings. Without
/// serde_json's `preserve_order` feature the map keys come back sorted,
/// which matches the legacy `names()` in anixie's `core.sh`
/// (`jq -r 'keys[]' | sort`).
pub(crate) fn theme_names() -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(THEMES_JSON) else {
        return Vec::new();
    };
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut names: Vec<String> = map.keys().cloned().collect();
    names.sort();
    names
}

pub(crate) fn current_theme() -> String {
    let p = state_dir().join("current");
    if let Ok(s) = std::fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    // fallback: state.json theme field
    let sp = state_dir().join("state.json");
    if let Ok(s) = std::fs::read_to_string(&sp)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&s)
            && let Some(theme) = value.get("theme").and_then(|t| t.as_str()) {
                return theme.to_string();
            }
    String::new()
}

pub(crate) fn state_file() -> PathBuf { state_dir().join("state.json") }
pub(crate) fn current_file() -> PathBuf { state_dir().join("current") }
#[allow(dead_code)]
pub(crate) fn mode_file() -> PathBuf { state_dir().join("mode") }
pub(crate) fn wallpaper_mode_file() -> PathBuf { state_dir().join("wallpaper-mode") }
pub(crate) fn cursor_file() -> PathBuf { state_dir().join("cursor-size") }
pub(crate) fn generated_file() -> PathBuf { state_dir().join("palette.json") }
#[allow(dead_code)]
pub(crate) fn live_link() -> PathBuf { state_dir().join("live") }
#[allow(dead_code)]
pub(crate) fn generations_dir() -> PathBuf { state_dir().join("generations") }

pub(crate) fn write_atomic(path: &PathBuf, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    if std::fs::rename(&tmp, path).is_err() {
        let r = std::fs::copy(&tmp, path);
        let _ = std::fs::remove_file(&tmp);
        r?;
    }
    Ok(())
}

pub(crate) fn palettes() -> serde_json::Value {
    serde_json::from_str(THEMES_JSON).unwrap_or(serde_json::Value::Null)
}

pub(crate) fn has_theme(name: &str) -> bool {
    name == "wallpaper" || palettes().get(name).map(|v| v.is_object()).unwrap_or(false)
}

pub(crate) fn palette_entry(name: &str) -> Option<serde_json::Value> {
    palettes().get(name).cloned()
}

pub(crate) fn theme_field(name: &str, key: &str) -> String {
    palette_entry(name)
        .and_then(|e| e.get(key).and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

pub(crate) fn mode_of(name: &str) -> String {
    let m = theme_field(name, "_mode");
    if m.is_empty() { "dark".to_string() } else { m }
}

#[allow(dead_code)]
pub(crate) fn current_mode() -> String {
    if current_theme() == "wallpaper" {
        if let Ok(s) = fs::read_to_string(generated_file())
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s)
                && let Some(m) = v.get("_mode").and_then(|x| x.as_str()) {
                    return m.to_string();
                }
        return "dark".to_string();
    }
    mode_of(&current_theme())
}

pub(crate) fn saved_wallpaper_mode() -> String {
    let mode = fs::read_to_string(wallpaper_mode_file())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    match mode.as_str() {
        "auto" | "smart" | "light" | "dark" => mode,
        _ => "auto".to_string(),
    }
}

pub(crate) fn read_state_json() -> Option<serde_json::Value> {
    let s = fs::read_to_string(state_file()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    if v.get("version").and_then(|x| x.as_i64()) == Some(1) {
        Some(v)
    } else {
        None
    }
}

pub(crate) fn migrated_state_json() -> serde_json::Value {
    let theme = fs::read_to_string(current_file())
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| has_theme(s) || s == "wallpaper")
        .unwrap_or_else(|| "tokyo-night".to_string());
    let source = if theme == "wallpaper" { "wallpaper" } else { "preset" };
    serde_json::json!({
        "version": 1,
        "source": source,
        "theme": theme,
        "followWallpaper": source == "wallpaper",
        "pinned": false,
        "favorites": [],
        "recent": [theme],
        "auto": {
            "schedule": {"enabled": false, "light": "07:00", "dark": "19:00"},
            "rotation": {"enabled": false, "interval": "30m", "shuffle": true}
        },
        "generation": ""
    })
}

pub(crate) fn state_ensure() -> Result<(), String> {
    if read_state_json().is_some() {
        return Ok(());
    }
    // existing invalid state file is renamed to .bad, like legacy migrate
    if state_file().exists() {
        let _ = fs::rename(state_file(), state_file().with_extension("json.bad"));
    }
    fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
    let json = migrated_state_json();
    write_atomic(&state_file(), &serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string()))
        .map_err(|e| e.to_string())
}

#[allow(dead_code)]
pub(crate) fn state_set_theme(name: &str, source: &str) -> Result<(), String> {
    state_ensure()?;
    let mut state = read_state_json().unwrap_or_else(migrated_state_json);
    if let Some(obj) = state.as_object_mut() {
        obj.insert("version".into(), serde_json::json!(1));
        obj.insert("source".into(), serde_json::json!(source));
        obj.insert("theme".into(), serde_json::json!(name));
        obj.insert("followWallpaper".into(), serde_json::json!(source == "wallpaper"));
        let mut recent: Vec<serde_json::Value> = obj
            .get("recent")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter(|v| v.as_str() != Some(name)).cloned().collect())
            .unwrap_or_default();
        recent.insert(0, serde_json::json!(name));
        recent.truncate(10);
        obj.insert("recent".into(), serde_json::Value::Array(recent));
    }
    write_atomic(&state_file(), &serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".to_string()))
        .map_err(|e| e.to_string())
}

#[allow(dead_code)]
pub(crate) fn state_set_generation(gen_dir: &str) -> Result<(), String> {
    state_ensure()?;
    let mut state = read_state_json().unwrap_or_else(migrated_state_json);
    if let Some(obj) = state.as_object_mut() {
        obj.insert("version".into(), serde_json::json!(1));
        obj.insert("generation".into(), serde_json::json!(gen_dir));
    }
    write_atomic(&state_file(), &serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".to_string()))
        .map_err(|e| e.to_string())
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

    #[test]
    fn theme_names_sorted() {
        let names = theme_names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn state_dir_is_anixie_theme() {
        let dir = state_dir();
        assert!(
            dir.ends_with(std::path::Path::new("anixie").join("theme")),
            "got: {}",
            dir.display()
        );
    }

}
