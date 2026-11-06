use crate::proc::{resolve_tool, run};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

const THEMES_JSON: &str = include_str!("../themes.json");

fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}


fn theme_names() -> Vec<String> {
    // parse keys from embedded JSON (simple: lines with quoted keys at top level)
    let mut names = Vec::new();
    // Use a tiny parser: find top-level object keys.
    // themes.json is {"name": {...}, ...}. Extract quoted keys before ':' at depth 1.
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut buf = String::new();
    let mut collecting_key = false;
    for ch in THEMES_JSON.chars() {
        if escape { escape = false; if collecting_key { buf.push(ch); } continue; }
        if ch == '\\' { escape = true; if collecting_key { } ; continue; }
        if in_str {
            if ch == '"' {
                in_str = false;
                if collecting_key && depth == 1 {
                    names.push(buf.clone());
                    buf.clear();
                    collecting_key = false;
                }
            } else {
                if collecting_key { buf.push(ch); }
            }
        } else {
            if ch == '"' {
                in_str = true;
                // a key starts if previous non-space is { or ,
                collecting_key = true;
                buf.clear();
            } else if ch == '{' {
                depth += 1;
                collecting_key = false;
            } else if ch == '}' {
                depth -= 1;
                collecting_key = false;
            } else if ch == ':' && collecting_key {
                // shouldn't happen; reset
                collecting_key = false;
                buf.clear();
            } else if !ch.is_whitespace() && ch != ',' {
                collecting_key = false;
            }
        }
    }
    names
}

fn current_theme() -> String {
    let p = state_dir().join("current");
    if let Ok(s) = std::fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    // fallback: state.json theme field
    let sp = state_dir().join("state.json");
    if let Ok(s) = std::fs::read_to_string(&sp) {
        // naive extract "theme": "name"
        if let Some(i) = s.find("\"theme\"") {
            let rest = &s[i..];
            if let Some(q1) = rest.find('"') {
                let after = &rest[q1+1..];
                // skip to next quote after key
                if let Some(q2) = after.find('"') {
                    let keyend = &after[q2+1..];
                    if let Some(q3) = keyend.find('"') {
                        let val = &keyend[q3+1..];
                        if let Some(q4) = val.find('"') {
                            return val[..q4].to_string();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

fn delegate(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let tool = resolve_tool("theme");
    let (code, out, err) = run(&tool, &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn presets_dir() -> PathBuf {
    if let Ok(v) = env::var("THEME_PRESETS_DIR") { return PathBuf::from(v); }
    let cfg = crate::config::get_str(&["theme", "presets_dir"], "");
    if !cfg.is_empty() { return PathBuf::from(cfg); }
    home_dir().join(".config/alexie/presets")
}

fn presets() -> String {
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

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        return "ERR theme: missing subcommand".to_string();
    }
    match args[0].as_str() {
        "list" => theme_names().join("\n"),
    "presets" => presets(),
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
}
