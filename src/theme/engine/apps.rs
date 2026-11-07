use super::color::color_accent_name;
use super::materialise::colour;
use crate::theme::palette;
use super::materialise::{StateLock, materialise};
use super::super::state::{self, live_link};
use crate::proc::{run, tool};
use std::env;
use std::path::PathBuf;

pub(crate) fn quickshell_bin() -> String { tool("QUICKSHELL", "quickshell") }
pub(crate) fn hyprctl_bin() -> String { tool("HYPRCTL", "hyprctl") }
fn gsettings_bin() -> String { tool("GSETTINGS", "gsettings") }
fn tmux_bin() -> String { tool("TMUX", "tmux") }

// ---------- tell_apps / hooks (port of apps.sh) ----------


pub(crate) fn generated_palette() -> serde_json::Value {
    std::fs::read_to_string(state::generated_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

pub(crate) fn tell_apps(name: &str) {
    let palette_path = state::live_link().join("palette.json");
    let qs = quickshell_bin();
    if name == "wallpaper" {
        let content = std::fs::read_to_string(&palette_path)
            .or_else(|_| std::fs::read_to_string(state::generated_file()))
            .unwrap_or_default()
            .replace('\n', "");
        let _ = run(&qs, &["ipc", "call", "theme", "palette", &content]);
    } else {
        let _ = run(&qs, &["ipc", "call", "theme", "set", name]);
    }
    let _ = run(&hyprctl_bin(), &["reload"]);
    let _ = run("pkill", &["-USR1", "-x", "kitty"]);

    let mode = std::fs::read_to_string(state::live_link().join("mode"))
        .or_else(|_| std::fs::read_to_string(state::mode_file()))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let (gtk_theme, icon_theme, color_scheme) = if mode == "light" {
        ("adw-gtk3", "Papirus-Light", "prefer-light")
    } else {
        ("adw-gtk3-dark", "Papirus-Dark", "prefer-dark")
    };
    let gs = gsettings_bin();
    let _ = run(&gs, &["set", "org.gnome.desktop.interface", "color-scheme", color_scheme]);
    let _ = run(&gs, &["set", "org.gnome.desktop.interface", "gtk-theme", gtk_theme]);
    let _ = run(&gs, &["set", "org.gnome.desktop.interface", "icon-theme", icon_theme]);
    let generated = generated_palette();
    let primary = colour(&generated, "primary");
    if let Some(accent) = color_accent_name(&primary) {
        let _ = run(&gs, &["set", "org.gnome.desktop.interface", "accent-color", &accent]);
    }

    // Equicord, best-effort like legacy
    let config_home = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", palette::home_dir().display()));
    let equicord = PathBuf::from(&config_home).join("Equicord");
    let live_discord = state::live_link().join("discord.css");
    if equicord.is_dir() && live_discord.exists() {
        let _ = std::fs::create_dir_all(equicord.join("themes"));
        let _ = std::fs::copy(&live_discord, equicord.join("themes").join("anixie.theme.css"));
        let settings = equicord.join("settings").join("settings.json");
        if settings.exists()
            && let Ok(s) = std::fs::read_to_string(&settings)
                && let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&s) {
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

    crate::theme::apply_cursor();
    let _ = run(&tmux_bin(), &["source-file", state::live_link().join("tmux.conf").to_string_lossy().as_ref()]);
    // Re-apply the saved wallpaper selection so videos and
    // per-output gradients follow the new palette (best-effort).
    let _ = crate::wallpaper::apply::apply();
}

pub(crate) fn run_hooks(name: &str) {
    let config_home = env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", palette::home_dir().display()));
    let hook_dir = PathBuf::from(&config_home).join("anixie").join("theme-hooks");
    if !hook_dir.is_dir() {
        return;
    }
    let mode = std::fs::read_to_string(state::live_link().join("mode"))
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
    if !state::has_theme(name) {
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
        let cur = state::current_theme();
        if state::has_theme(&cur) || cur == "wallpaper" { cur } else { "tokyo-night".to_string() }
    };
    let state_theme = state::read_state_json()
        .and_then(|v| v.get("theme").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    let renderer = std::fs::read_to_string(state::live_link().join("renderer"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let needs_materialise = !live_link().join("palette.json").exists()
        || state_theme != name
        || renderer.is_empty();
    if needs_materialise
        && let Err(e) = materialise(&name) {
            return format!("ERR {}", e);
        }
    drop(_lock);
    tell_apps(&name);
    run_hooks(&name);
    "OK".to_string()
}

