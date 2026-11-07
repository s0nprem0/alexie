use crate::proc::{resolve_tool, run};

pub fn handle(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let tool = resolve_tool("shortcuts");
    let (code, out, err) = run(&tool, &a);
    if code == 0 {
        if out.trim().is_empty() {
            "OK".to_string()
        } else {
            out.trim_end().to_string()
        }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}
