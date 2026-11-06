use crate::proc::{resolve_tool, run, shell_quote};

pub fn handle(_args: &[String]) -> String {
    let tool = resolve_tool("emoji");
    let (code, out, err) = run(&tool, &[]);
    if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
    else { format!("ERR (exit {}) {}", code, err.trim()) }
}
