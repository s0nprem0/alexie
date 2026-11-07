//! Menu: open/list/launch via quickshell IPC.

use crate::proc::{run, quickshell as qs};

fn call(args: &[&str]) -> (i32, String, String) {
    let mut full: Vec<&str> = vec!["ipc", "call", "menu"];
    full.extend_from_slice(args);
    run(&qs(), &full)
}

pub fn handle(args: &[String]) -> String {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return "usage: menu [apps | --only GROUP | --list [GROUP] | NAME]".to_string();
    }
    match args.first().map(|s| s.as_str()) {
        None => {
            let (code, _, err) = call(&["open"]);
            if code == 0 { "OK".to_string() } else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
        Some("apps") => {
            let (code, _, err) = call(&["only", "Applications"]);
            if code == 0 { "OK".to_string() } else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
        Some("--only") => {
            if args.len() < 2 {
                return "ERR menu: --only needs a group name".to_string();
            }
            let (code, _, err) = call(&["only", &args[1]]);
            if code == 0 { "OK".to_string() } else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
        Some("--list") => {
            let group = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let (code, out, err) = if group.is_empty() {
                call(&["list", ""])
            } else {
                call(&["list", group])
            };
            if code == 0 { out.trim_end().to_string() } else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
        Some(name) => {
            let (code, reply, err) = call(&["launch", name]);
            if code != 0 {
                return format!("ERR (exit {}) {}", code, err.trim());
            }
            let reply = reply.trim();
            if reply == "ok" { "OK".to_string() } else { format!("ERR {}", reply) }
        }
    }
}
