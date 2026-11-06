use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const THEMES_JSON: &str = include_str!("../themes.json");

fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}


fn resolve_tool(name: &str) -> String {
    // When the daemon is enabled, bash tools are renamed to *-legacy. Prefer
    // the plain name, fall back to the legacy one so delegation keeps working.
    let check = format!("command -v {} >/dev/null 2>&1", name);
    if std::process::Command::new("sh").args(["-c", &check]).status().map(|s| s.success()).unwrap_or(false) {
        return name.to_string();
    }
    let legacy = format!("{}-legacy", name);
    let check2 = format!("command -v {} >/dev/null 2>&1", legacy);
    if std::process::Command::new("sh").args(["-c", &check2]).status().map(|s| s.success()).unwrap_or(false) {
        return legacy;
    }
    name.to_string()
}

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(resolve_tool(cmd)).args(args).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            (code, out, err)
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
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
    let tool = crate::resolve_tool("theme");
    let (code, out, err) = run(&tool, &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        return "ERR theme: missing subcommand".to_string();
    }
    match args[0].as_str() {
        "list" => theme_names().join("\n"),
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
            delegate(&[format!("set"), name.clone()])
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