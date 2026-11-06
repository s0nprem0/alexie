use std::process::Command;

pub fn resolve_tool(name: &str) -> String {
    let legacy = format!("{}-legacy", name);
    let check2 = format!("command -v {} >/dev/null 2>&1", legacy);
    if Command::new("sh").args(["-c", &check2]).status().map(|s| s.success()).unwrap_or(false) {
        return legacy;
    }
    let check = format!("command -v {} >/dev/null 2>&1", name);
    if Command::new("sh").args(["-c", &check]).status().map(|s| s.success()).unwrap_or(false) {
        return name.to_string();
    }
    name.to_string()
}

pub fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(resolve_tool(cmd)).args(args).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            (code, out, err)
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

/// Run a command with `stdin_data` fed to its stdin.
///
/// Legacy `pick` reads its entries from stdin and exits as soon as
/// the shell has taken the request — the shell, not this process,
/// runs `--on-select` later. Callers must not wait for a selection
/// on stdout. `Command::output()` (used by `run`) discards stdin,
/// so piped entries were lost.
pub fn run_with_stdin(cmd: &str, args: &[&str], stdin_data: &str) -> (i32, String, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = match Command::new(resolve_tool(cmd))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (-1, String::new(), e.to_string()),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_data.as_bytes());
        // stdin drops here, signalling EOF to the child
    }
    match child.wait_with_output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            (code, out, err)
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

pub fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
}


/// Run a shell snippet for its exit status only, with stdio nulled.
pub fn run_sh_status(script: &str) -> i32 {
    use std::process::Stdio;
    Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.code().unwrap_or(-1))
        .unwrap_or(-1)
}

/// A unique temp file path under the system temp dir.
///
/// The old callers used `/tmp/anixie-<kind>-{pid}.png` — PID-only, so
/// concurrent daemon calls (the daemon is now threaded per connection)
/// or a stale file from a killed process collide and clobber each
/// other. Mix in a monotonic counter and the current time so each
/// call in a process gets a distinct name.
pub fn tmp_file(prefix: &str, ext: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = if ext.is_empty() {
        format!("anixie-{}-{}-{}-{}", prefix, std::process::id(), nanos, n)
    } else {
        format!("anixie-{}-{}-{}-{}.{}", prefix, std::process::id(), nanos, n, ext)
    };
    std::env::temp_dir().join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_with_stdin_feeds_input() {
        // `cat` echoes its stdin, proving the data actually reaches
        // the child (the old `run` discards stdin).
        let (code, out, _err) = run_with_stdin("cat", &[], "hello\nworld\n");
        assert_eq!(code, 0);
        assert_eq!(out, "hello\nworld\n");
    }

    #[test]
    fn shell_quote_basic() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn tmp_file_unique_per_call() {
        let a = tmp_file("test", "png");
        let b = tmp_file("test", "png");
        assert_ne!(a, b);
        assert!(a.to_string_lossy().ends_with(".png"));
        assert!(a.to_string_lossy().contains("anixie-test-"));
    }
}
