use std::env;
use std::fs;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn resolve_tool(name: &str) -> String {
    let check = format!("command -v {} >/dev/null 2>&1", name);
    if Command::new("sh").args(["-c", &check]).status().map(|s| s.success()).unwrap_or(false) {
        return name.to_string();
    }
    let legacy = format!("{}-legacy", name);
    let check2 = format!("command -v {} >/dev/null 2>&1", legacy);
    if Command::new("sh").args(["-c", &check2]).status().map(|s| s.success()).unwrap_or(false) {
        return legacy;
    }
    name.to_string()
}

fn runtime_dir() -> std::path::PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let st = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        });
        st
    });
    std::path::PathBuf::from(rt)
}

pub fn handle(_args: &[String]) -> String {
    let last_capture_file = runtime_dir().join("last-capture");
    if !last_capture_file.exists() {
        let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", "No recent capture to share."]);
        return "ERR no recent capture".to_string();
    }
    let last = fs::read_to_string(&last_capture_file).unwrap_or_default().trim().to_string();
    if last.is_empty() || !std::path::Path::new(&last).exists() {
        let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", "Recent capture not found."]);
        return "ERR recent capture not found".to_string();
    }
    // delegate interactive picker to bash share (complex pick menu)
    let tool = resolve_tool("share");
    let (code, out, err) = run(&tool, &[]);
    if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
    else { format!("ERR (exit {}) {}", code, err.trim()) }
}
