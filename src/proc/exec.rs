//! Process execution: capture stdout/stderr, feed stdin (text and
//! bytes), redirect stdout to file, and sh-status check.

use super::tool::resolve_tool;
use std::process::Command;

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
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(stdin_data.as_bytes())
    {
        let _ = child.kill();
        let _ = child.wait();
        return (-1, String::new(), format!("write stdin: {e}"));
    }
    // stdin drops here, signalling EOF to the child
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

/// Like `run_with_stdin`, but for binary data (clipboard image
/// selections): a `&str` round-trip would corrupt non-UTF8 bytes.
pub fn run_bytes_with_stdin(cmd: &str, args: &[&str], data: &[u8]) -> (i32, String, String) {
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
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(data)
    {
        let _ = child.kill();
        let _ = child.wait();
        return (-1, String::new(), format!("write stdin: {e}"));
    }
    // stdin drops here, signalling EOF to the child
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

/// Like `run`, but kills the child after `timeout` and returns -1.
/// For quickshell IPC calls: an unanswered shell must not leave the
/// daemon thread (and the user's frozen overlay) hanging forever.
pub fn run_timeout(
    cmd: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> (i32, String, String) {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = match Command::new(resolve_tool(cmd))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (-1, String::new(), e.to_string()),
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut v = String::new();
                        let _ = s.read_to_string(&mut v);
                        v
                    })
                    .unwrap_or_default();
                let err = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut v = String::new();
                        let _ = s.read_to_string(&mut v);
                        v
                    })
                    .unwrap_or_default();
                return (status.code().unwrap_or(-1), out, err);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (
                        -1,
                        String::new(),
                        format!("timed out after {}s", timeout.as_secs()),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return (-1, String::new(), e.to_string()),
        }
    }
}

/// Shell-quote `s` as a single POSIX word (`'…'\''…'`).
/// Used for static args baked into `pick --on-select` templates at
/// build time (e.g. transcode input paths). The substituted `{}` value
/// is quoted template-side as `'{}'`; the shell (quickshell) must
/// treat the substituted word as data, so picker values must stay free
/// of `'` — handlers validate this (see emoji `apply`).
pub fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
