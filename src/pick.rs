use std::env;
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

pub fn handle(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let tool = resolve_tool("pick");
    let (code, out, err) = run(&tool, &a);
    if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
    else { format!("ERR (exit {}) {}", code, err.trim()) }
}
