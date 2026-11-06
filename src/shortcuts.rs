use std::env;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn hyprctl() -> String { "hyprctl".to_string() }
fn jq() -> String { "jq".to_string() }

pub fn handle(_args: &[String]) -> String {
    // shortcuts: hyprctl binds -j | jq transform | pick. Delegate to bash shortcuts
    // (complex jq transform; keep correctness).
    let tool = if Command::new("sh").args(["-c","command -v shortcuts >/dev/null 2>&1"]).status().map(|s|s.success()).unwrap_or(false) {
        "shortcuts".to_string()
    } else { "shortcuts-legacy".to_string() };
    let (code, out, err) = run(&tool, &[]);
    if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
    else { format!("ERR (exit {}) {}", code, err.trim()) }
}
