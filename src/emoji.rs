use std::env;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

pub fn handle(_args: &[String]) -> String {
    // delegate to bash emoji (complex awk + pick + wtype)
    let (code, out, err) = run("emoji", &[]);
    if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
    else { format!("ERR (exit {}) {}", code, err.trim()) }
}
RUST
echo done