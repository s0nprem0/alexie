use super::apps::{hyprctl_bin, quickshell_bin};
use super::super::state;
use super::color::{contrast, palette_ok_file};
use crate::proc;
use std::env;
use std::path::PathBuf;

// ---------- doctor (port of doctor_theme) ----------

pub(crate) fn doctor() -> String {
    let _ = state::state_ensure();
    let mut failures: i64 = 0;
    let mut warnings: i64 = 0;
    let mut out = String::new();

    let live = state::live_link();
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
                if let Ok(s) = std::fs::read_to_string(entry.path())
                    && s.lines().any(|line| {
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
        if unresolved {
            out.push_str("invalid  unresolved placeholders in the live generation\n");
            failures += 1;
        }

        if live.join("palette.json").exists()
            && let Ok(s) = std::fs::read_to_string(live.join("palette.json"))
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                    for (fg_tok, bg_tok) in [
                        ("muted", "surface"),
                        ("muted", "background"),
                        ("fgLight", "surface"),
                        ("fgLight", "background"),
                    ] {
                        let fg_c = v.get(fg_tok).and_then(|x| x.as_str()).unwrap_or("");
                        let bg_c = v.get(bg_tok).and_then(|x| x.as_str()).unwrap_or("");
                        if let (Some(fg_c), Some(bg_c)) = (Some(fg_c), Some(bg_c))
                            && let Some(ratio) = contrast(fg_c, bg_c)
                                && ratio < 4.5 {
                                    out.push_str(&format!("warn     {} vs {} contrast {:.2} < 4.5\n", fg_tok, bg_tok, ratio));
                                    warnings += 1;
                                }
                    }
                }
    }

    let config_home = env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", super::super::palette::home_dir().display()));
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
        if proc::find_in_path(&bin, &env::var("PATH").unwrap_or_default()).is_none()
            && which(&bin).is_none()
        {
            out.push_str(&format!("warn     {} not found in PATH\n", name));
            warnings += 1;
        }
    }

    let wp = state::state_dir().join("wallpapers.json");
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

