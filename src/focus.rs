use crate::proc::{resolve_tool, run, shell_quote};
use std::env;

fn qs() -> String { env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string()) }


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
