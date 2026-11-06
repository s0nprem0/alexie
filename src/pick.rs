use crate::proc::run_with_stdin;

/// Run the generic picker with `stdin_data` as its entries.
///
/// Legacy `pick` reads entries from stdin (one per line) and exits
/// immediately; the shell runs `--on-select` later. The old
/// implementation used `Command::output()` via `run()`, which
/// discards stdin, so piped entries were lost and pick died with
/// "nothing to pick from".
pub fn handle_with_stdin(args: &[String], stdin_data: &str) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, out, err) = run_with_stdin("pick", &a, stdin_data);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

#[allow(dead_code)]
pub fn handle(args: &[String]) -> String {
    // No stdin forwarded on the DBus/back-compat path; the socket
    // protocol forwards piped stdin via handle_with_stdin.
    handle_with_stdin(args, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_unknown_pick_arg_fails_fast() {
        // Unknown pick flags are rejected by the legacy pick script
        // (exit != 0) if it is installed; without it, spawn fails.
        // Either way the result is an ERR string, not a hang.
        // We do not assert a specific message because pick may not
        // exist in the test sandbox.
        let out = handle(&["--__bogus_flag__".to_string()]);
        assert!(out.starts_with("ERR"), "got: {out}");
    }
}
