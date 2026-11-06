use crate::proc::run;
use std::env;
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
    if let Ok(s) = std::fs::read_to_string(&sp) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&s) {
            if let Some(theme) = value.get("theme").and_then(|t| t.as_str()) {
                return theme.to_string();
            }
        }
    }
    String::new()
}

pub(crate) fn delegate(args: &[String]) -> String {
    // Delegate heavy ops to the legacy bash implementation explicitly.
    // Using resolve_tool("theme") here would re-enter alexie itself via
    // the `theme` shim (`alexie theme "$@"`) when daemon mode is on,
    // recursing forever. `run("theme-legacy")` goes through
    // resolve_tool("theme-legacy"), which prefers a real legacy binary.
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, out, err) = run("theme-legacy", &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else if err.trim() == "No such file or directory" || code == -1 {
        // Command::new fails with -1/ENOENT when no legacy binary exists.
        format!(
            "ERR theme-legacy not found: subcommand `{}` needs the legacy bash theme (enable anixie.daemon, which renames it to theme-legacy)",
            args.first().map(|s| s.as_str()).unwrap_or("")
        )
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
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

    #[test]
    fn delegate_unknown_subcommand_fails_fast() {
        let out = delegate(&["__bogus_subcommand__".to_string()]);
        assert!(out.starts_with("ERR"), "got: {out}");
    }
}
