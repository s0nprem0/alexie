use crate::proc::{resolve_tool, run, shell_quote};
use std::env;
use std::fs;
use std::process::Command;

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
