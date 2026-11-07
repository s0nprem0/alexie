use crate::proc::{run, quickshell as qs};

fn usage() -> String {
    "focus — a timed distraction block\n\nUsage:\n  focus [status]\n  focus start [MINUTES]\n  focus stop\n  focus extend [MINUTES]\n".to_string()
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "-h" | "--help" | "help" => usage(),
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
            // Fully ported (no focus-legacy): running a binary named
            // `focus` here would re-enter alexie via the focus shim
            // and recurse forever. Match legacy: unknown -> error.
            format!("ERR unknown subcommand: {}", sub)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }

    #[test]
    fn help_returns_usage() {
        assert!(handle(&["--help".to_string()]).contains("focus start"));
    }
}
