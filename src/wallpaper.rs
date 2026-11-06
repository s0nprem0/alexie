use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}

fn current_theme() -> String {
    let p = state_dir().join("current");
    if let Ok(s) = fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    String::new()
}


fn resolve_tool(name: &str) -> String {
    // Prefer *-legacy (bash impl) when daemon is enabled, so delegation never
    // recurses through the alexie shim.
    let legacy = format!("{}-legacy", name);
    let check2 = format!("command -v {} >/dev/null 2>&1", legacy);
    if std::process::Command::new("sh").args(["-c", &check2]).status().map(|s| s.success()).unwrap_or(false) {
        return legacy;
    }
    let check = format!("command -v {} >/dev/null 2>&1", name);
    if std::process::Command::new("sh").args(["-c", &check]).status().map(|s| s.success()).unwrap_or(false) {
        return name.to_string();
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

fn delegate(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let tool = crate::resolve_tool("wallpaper");
    let (code, out, err) = run(&tool, &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        // default: pick
        return delegate(args);
    }
    match args[0].as_str() {
        "current" => {
            let theme = current_theme();
            let file = state_dir().join(format!("wallpaper.{}", theme));
            if let Ok(s) = fs::read_to_string(&file) {
                let s = s.trim().to_string();
                if !s.is_empty() { return s; }
            }
            // fallback: wallpapers.json
            let wp = state_dir().join("wallpapers.json");
            if let Ok(s) = fs::read_to_string(&wp) {
                // naive: try jq
                let filter = format!(r#".themes["{}"].fallback.path // empty"#, theme);
                let script = format!("printf '%s' {} | jq -r '{}'", shell_quote(&s), filter);
                let (code, out, _) = run("sh", &["-c", &script]);
                if code == 0 {
                    let out = out.trim();
                    if !out.is_empty() && out != "null" { return out.to_string(); }
                }
            }
            "generated".to_string()
        }
        "status" => delegate(args),
        "list" | "library" => delegate(args),
        // heavy: delegate
        _ => delegate(args),
    }
}

fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_basic() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn current_theme_no_panic() {
        // Just ensure it doesn't panic in test env.
        let _ = current_theme();
    }
}
