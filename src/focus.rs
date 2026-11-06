use std::env;
use std::process::Command;

fn qs() -> String { env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string()) }


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
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

pub fn handle(args: &[String]) -> String {
    let sub = args.get(0).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "" | "status" => {
            let (code, out, _) = run(&qs(), &["ipc", "call", "focus", "status"]);
            if code == 0 { out.trim().to_string() } else { "off".to_string() }
        }
        "start" => {
            let m = args.get(1).cloned().unwrap_or_default();
            let (code, out, err) = if m.is_empty() {
                run(&qs(), &["ipc", "call", "focus", "start", ""])
            } else {
                run(&qs(), &["ipc", "call", "focus", "start", &m])
            };
            if code != 0 { format!("ERR {}", err.trim()) } else { if out.trim().is_empty() { "OK".to_string() } else { out.trim().to_string() } }
        }
        "stop" => {
            let (code, out, err) = run(&qs(), &["ipc", "call", "focus", "stop"]);
            if code != 0 { format!("ERR {}", err.trim()) } else { if out.trim().is_empty() { "OK".to_string() } else { out.trim().to_string() } }
        }
        "extend" => {
            let m = args.get(1).cloned().unwrap_or_default();
            let (code, out, err) = if m.is_empty() {
                run(&qs(), &["ipc", "call", "focus", "extend", ""])
            } else {
                run(&qs(), &["ipc", "call", "focus", "extend", &m])
            };
            if code != 0 { format!("ERR {}", err.trim()) } else { if out.trim().is_empty() { "OK".to_string() } else { out.trim().to_string() } }
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run("focus", &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}
