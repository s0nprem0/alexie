//! State-modifying theme subcommands: info, favorite, pin,
//! wallpaper-mode, cursor-size. `apply_cursor` is pub(crate) for
//! engine apps.

use super::state::{self, current_theme, has_theme, mode_of, palette_entry, saved_wallpaper_mode, state_dir, state_ensure, state_file, theme_field, wallpaper_mode_file, write_atomic};
use crate::proc::run;

pub(crate) fn info_command(args: &[String]) -> String {
    if args.len() > 1 {
        return "ERR usage: theme info [NAME]".to_string();
    }
    let name = args.first().cloned().unwrap_or_else(current_theme);
    if name == "wallpaper" {
        let mut out = "Theme: wallpaper (generated)\n".to_string();
        if state::generated_file().exists() {
            let mode = std::fs::read_to_string(state::generated_file())
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("_mode").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| "dark".to_string());
            out.push_str(&format!("Mode:   {}\n", mode));
            out.push_str("Source: matugen from wallpaper");
        }
        return out;
    }
    if !has_theme(&name) {
        return format!("ERR no theme called \"{}\" (try: theme list)", name);
    }
    let mode = mode_of(&name);
    let family = theme_field(&name, "_family");
    let pair = theme_field(&name, "_pair");
    let variant = theme_field(&name, "_variant");
    let mut out = format!("Theme:   {}\nMode:    {}\n", name, mode);
    if !family.is_empty() { out.push_str(&format!("Family:  {}\n", family)); }
    if !variant.is_empty() { out.push_str(&format!("Variant: {}\n", variant)); }
    if !pair.is_empty() { out.push_str(&format!("Pair:    {}\n", pair)); }
    out.push('\n');
    // colours, like legacy
    if let Some(entry) = palette_entry(&name) {
        for key in ["background", "surface", "primary", "accent", "foreground", "muted", "success", "yellow", "danger"] {
            let val = entry.get(key).and_then(|v| v.as_str()).unwrap_or("");
            out.push_str(&format!("  {}\t{}\n", key, val));
        }
    }
    out.trim_end().to_string()
}

pub(crate) fn favorite_command(action: &str, name: &str) -> String {
    // Usage errors first: they must not depend on the state
    // directory being readable (the Nix sandbox has no writable
    // HOME, and a typo should fail the same everywhere).
    if !matches!(action, "add" | "remove" | "toggle") {
        return "ERR favorite must be add, remove or toggle".to_string();
    }
    if !has_theme(name) {
        return format!("ERR no theme called \"{}\" (try: theme list)", name);
    }
    if state_ensure().is_err() {
        return "ERR could not read the theme state".to_string();
    }
    let mut state = state::read_state_json().unwrap_or_else(state::migrated_state_json);
    let mut favs: Vec<String> = state
        .get("favorites")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    match action {
        "add" => {
            if !favs.contains(&name.to_string()) { favs.push(name.to_string()); favs.sort(); favs.dedup(); }
        }
        "remove" => favs.retain(|f| f != name),
        "toggle" => {
            if favs.contains(&name.to_string()) { favs.retain(|f| f != name); }
            else { favs.push(name.to_string()); favs.sort(); favs.dedup(); }
        }
        _ => return "ERR favorite must be add, remove or toggle".to_string(),
    }
    favs.sort();
    favs.dedup();
    if let Some(obj) = state.as_object_mut() {
        obj.insert("version".into(), serde_json::json!(1));
        obj.insert("favorites".into(), serde_json::json!(favs));
    }
    let tmp = state_file().with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".to_string())).is_err() {
        return "ERR could not update favorites".to_string();
    }
    if std::fs::rename(&tmp, state_file()).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return "ERR could not update favorites".to_string();
    }
    "OK".to_string()
}

pub(crate) fn pin_command(value: bool) -> String {
    if state_ensure().is_err() {
        return "ERR could not read the theme state".to_string();
    }
    let mut state = state::read_state_json().unwrap_or_else(state::migrated_state_json);
    if let Some(obj) = state.as_object_mut() {
        obj.insert("version".into(), serde_json::json!(1));
        obj.insert("pinned".into(), serde_json::json!(value));
    }
    let tmp = state_file().with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".to_string())).is_err() {
        return "ERR could not update the pin".to_string();
    }
    if std::fs::rename(&tmp, state_file()).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return "ERR could not update the pin".to_string();
    }
    "OK".to_string()
}

pub(crate) fn wallpaper_mode_command(args: &[String]) -> String {
    if args.is_empty() {
        return saved_wallpaper_mode();
    }
    if args.len() != 1 {
        return "ERR usage: theme wallpaper-mode [auto|light|dark]".to_string();
    }
    match args[0].as_str() {
        "auto" | "light" | "dark" => {
            if write_atomic(&wallpaper_mode_file(), &format!("{}\n", args[0])).is_err() {
                return "ERR could not write wallpaper-mode".to_string();
            }
            "OK".to_string()
        }
        _ => "ERR wallpaper-mode must be auto, light or dark".to_string(),
    }
}

pub(crate) fn apply_cursor() {
    let size = std::fs::read_to_string(state::cursor_file())
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| (8..=128).contains(n))
        .unwrap_or_else(|| std::env::var("CURSOR_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(24));
    // Best-effort, like legacy: a missing gsettings/hyprctl must
    // not fail the theme switch.
    let _ = run("gsettings", &["set", "org.gnome.desktop.interface", "cursor-size", &size.to_string()]);
    let cursor_theme = std::env::var("CURSOR_THEME").unwrap_or_else(|_| "Adwaita".to_string());
    let _ = run("hyprctl", &["setcursor", &cursor_theme, &size.to_string()]);
}

pub(crate) fn cursor_size_command(value: &str) -> String {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
        return "ERR cursor-size must be a positive integer".to_string();
    }
    let n: i64 = match value.parse() {
        Ok(n) => n,
        Err(_) => return "ERR cursor-size must be a positive integer".to_string(),
    };
    if !(8..=128).contains(&n) {
        return "ERR cursor-size must be between 8 and 128".to_string();
    }
    let _ = std::fs::create_dir_all(state_dir());
    if std::fs::write(state::cursor_file(), format!("{}\n", n)).is_err() {
        return "ERR could not write cursor-size".to_string();
    }
    apply_cursor();
    "OK".to_string()
}


