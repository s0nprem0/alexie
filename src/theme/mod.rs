//! Theme: palette listing and switching.
//!
//! Split from a single 680-line theme.rs for readability:
//! - `state` — state dir, theme names, current theme, legacy delegate
//! - `palette` — presets, matugen palette, wallpaper switch
//! - `vars` — colour vars build/cache, `vars`/`render` commands, presets list
//!
//! `handle()` below is just the dispatcher over those.

pub mod state;
pub mod palette;
pub mod vars;

use state::{current_theme, delegate, state_dir, theme_names};

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
