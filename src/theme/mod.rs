//! Theme: palette listing and switching.
//!
//! Split from a single 680-line theme.rs for readability:
//! - `state` — state dir, theme names, current theme, state.json
//! - `palette` — presets, matugen palette, wallpaper switch
//! - `vars` — colour vars build/cache, `vars`/`render` commands, presets list
//! - `engine` — native materialise/generate/set/apply/auto/doctor
//! - `commands` — info/favorite/pin/wallpaper-mode/cursor-size
//!
//! `handle()` below is just the dispatcher over those. All documented
//! subcommands are native; unknown ones are rejected outright.

pub mod state;
pub mod palette;
pub mod vars;
pub mod engine;
pub mod commands;

use commands::{cursor_size_command, favorite_command, info_command, pin_command, wallpaper_mode_command};
use state::{current_theme, state_ensure, state_file, theme_names};

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
        // Unknown subcommands are rejected outright; there is no
        // legacy bash theme left to delegate to.
        _ => format!("ERR unknown theme subcommand: {}", args[0]),
    }
}


// Re-export for callers that used crate::theme::apply_cursor (engine apps).
pub(crate) use commands::apply_cursor;

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

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus_subcommand__".to_string()]);
        assert!(out.starts_with("ERR unknown theme subcommand:"), "got: {out}");
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
