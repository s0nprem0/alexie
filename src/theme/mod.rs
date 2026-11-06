//! Theme: palette listing and switching.
//!
//! Split from a single 680-line theme.rs for readability:
//! - `state` — state dir, theme names, current theme, state.json, legacy delegate
//! - `palette` — presets, matugen palette, wallpaper switch
//! - `vars` — colour vars build/cache, `vars`/`render` commands, presets list
//! - `engine` — native materialise/generate/set/apply/auto/doctor
//!
//! `handle()` below is just the dispatcher over those. All documented
//! subcommands are native now; `delegate()` is only the fallback for
//! unknown subcommands, with a clear theme-legacy-not-found message.

pub mod state;
pub mod palette;
pub mod vars;
pub mod engine;

use state::{current_theme, delegate, has_theme, mode_of, palette_entry, saved_wallpaper_mode, state_dir, state_ensure, state_file, theme_field, theme_names, wallpaper_mode_file, write_atomic};
use crate::proc::run;

pub fn handle(args: &[String]) -> String {
    // Bare `alexie theme` lists themes — like `power`, `screentime`, etc.
    // default to a read-only view instead of erroring, and like the
    // legacy `theme` which printed the picker list. Users can still
    // use `theme list|current|next|prev|state|set …` explicitly.
    if args.is_empty() {
        return theme_names().join("\n");
    }
    match args[0].as_str() {
        "list" => match &args[1..] {
            [] => theme_names().join("\n"),
            [flag] if flag == "--json" => {
                serde_json::to_string(&theme_names()).unwrap_or_else(|_| "[]".to_string())
            }
            _ => "ERR usage: theme list [--json]".to_string(),
        },
    "presets" => vars::presets(),
    "switch" => {
        let wp = args.get(1).cloned().unwrap_or_default();
        let mode = args.get(2).cloned().unwrap_or_else(|| "auto".to_string());
        palette::switch_theme(&wp, &mode)
    }
    "vars" => vars::theme_vars(&args[1..]),
    "render" => vars::theme_render(&args[1..]),
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
            engine::set_theme(name)
        }
        "state" => {
            match state_ensure() {
                Ok(()) => std::fs::read_to_string(state_file()).map(|s| s.trim().to_string()).unwrap_or_else(|_| "{}".to_string()),
                Err(e) => format!("ERR could not initialise {}: {}", state_file().display(), e),
            }
        }
        "migrate" => {
            // Import old separate state files: delete the v1 state
            // (state_ensure will recreate it from `current`), then print it.
            // If state.json is already valid v1, migrate is a no-op like legacy.
            if state::read_state_json().is_none() {
                match state_ensure() {
                    Ok(()) => std::fs::read_to_string(state_file()).map(|s| s.trim().to_string()).unwrap_or_else(|_| "{}".to_string()),
                    Err(e) => format!("ERR could not migrate the theme state: {}", e),
                }
            } else {
                std::fs::read_to_string(state_file()).map(|s| s.trim().to_string()).unwrap_or_else(|_| "{}".to_string())
            }
        }
        "export" => {
            let name = current_theme();
            if name == "wallpaper" {
                return "# the live theme is generated from a wallpaper\n# choose a preset, then run: anixie theme export".to_string();
            }
            let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
            format!("anixie.theme.theme = \"{}\";", escaped)
        }
        "info" => info_command(&args[1..]),
        "favorite" => {
            if args.len() != 3 {
                return "ERR usage: theme favorite add|remove|toggle NAME".to_string();
            }
            favorite_command(&args[1], &args[2])
        }
        "pin" => pin_command(true),
        "unpin" => pin_command(false),
        "wallpaper-mode" => wallpaper_mode_command(&args[1..]),
        "cursor-size" => {
            if args.len() != 2 {
                return "ERR usage: theme cursor-size N".to_string();
            }
            cursor_size_command(&args[1])
        }
        "set" => {
            if args.len() != 2 {
                return "ERR usage: theme set NAME".to_string();
            }
            engine::set_theme(&args[1])
        }
        "wallpaper" => {
            if args.len() > 3 {
                return "ERR usage: theme wallpaper [FILE] [auto|light|dark]".to_string();
            }
            let wp = args.get(1).cloned().unwrap_or_default();
            let mode = args.get(2).cloned().unwrap_or_default();
            engine::wallpaper_theme(&wp, &mode)
        }
        "auto" => engine::apply_auto(),
        "apply" => engine::apply_theme(),
        "doctor" => engine::doctor(),
        // Unknown subcommands fall back to legacy with a clear
        // not-found message, instead of claiming native support.
        _ => delegate(args),
    }
}

fn info_command(args: &[String]) -> String {
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

fn favorite_command(action: &str, name: &str) -> String {
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

fn pin_command(value: bool) -> String {
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

fn wallpaper_mode_command(args: &[String]) -> String {
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

fn apply_cursor() {
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

fn cursor_size_command(value: &str) -> String {
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


#[cfg(test)]
mod tests {
    use super::*;
    use state::theme_names;

    #[test]
    fn theme_bare_defaults_to_list() {
        let bare = handle(&[]);
        assert!(!bare.starts_with("ERR"));
        assert_eq!(bare, theme_names().join("\n"));
        assert_eq!(bare, handle(&["list".to_string()]));
    }

    #[test]
    fn theme_list_json() {
        let out = handle(&["list".to_string(), "--json".to_string()]);
        assert!(!out.starts_with("ERR"), "got: {out}");
        let parsed: Vec<String> = serde_json::from_str(&out).expect("list --json is a JSON array");
        assert_eq!(parsed, theme_names());
    }

    #[test]
    fn theme_list_rejects_unknown_flag() {
        let out = handle(&["list".to_string(), "--bogus".to_string()]);
        assert!(out.starts_with("ERR usage: theme list"), "got: {out}");
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn info_known_theme() {
        let out = handle(&["info".to_string(), "tokyo-night".to_string()]);
        assert!(out.contains("Theme:   tokyo-night"), "got: {out}");
        assert!(out.contains("Mode:    dark"), "got: {out}");
        assert!(out.contains("background"), "got: {out}");
    }

    #[test]
    fn info_unknown_theme_is_err() {
        let out = handle(&["info".to_string(), "no-such-theme".to_string()]);
        assert!(out.starts_with("ERR no theme called"), "got: {out}");
    }

    #[test]
    fn export_uses_current_theme_name() {
        // current may be the real system theme; export must always
        // produce a Nix snippet or the wallpaper comment, never ERR.
        let out = handle(&["export".to_string()]);
        assert!(
            out.starts_with("anixie.theme.theme = ") || out.starts_with("# the live theme"),
            "got: {out}"
        );
    }

    #[test]
    fn favorite_and_pin_usage() {
        assert!(handle(&["favorite".to_string()]).starts_with("ERR usage: theme favorite"));
        assert!(handle(&["favorite".to_string(), "bogus".to_string(), "tokyo-night".to_string()])
            .starts_with("ERR favorite must be"));
        assert!(handle(&["favorite".to_string(), "add".to_string(), "no-such".to_string()])
            .starts_with("ERR no theme called"));
        assert!(handle(&["wallpaper-mode".to_string(), "bogus".to_string()])
            .starts_with("ERR wallpaper-mode must be"));
        assert!(handle(&["cursor-size".to_string(), "abc".to_string()])
            .starts_with("ERR cursor-size"));
        assert!(handle(&["cursor-size".to_string(), "999".to_string()])
            .starts_with("ERR cursor-size must be between"));
    }

    #[test]
    fn cursor_size_validates_range() {
        // Pure validation happens before any file write that would
        // need the real state dir — out-of-range must fail.
        assert!(handle(&["cursor-size".to_string(), "7".to_string()])
            .starts_with("ERR cursor-size must be between"));
        assert!(handle(&["cursor-size".to_string(), "129".to_string()])
            .starts_with("ERR cursor-size must be between"));
    }
}
