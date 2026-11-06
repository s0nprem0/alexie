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
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(data);
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

#[allow(dead_code)]
pub fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
}


/// Run a shell snippet for its exit status only, with stdio nulled.
#[allow(dead_code)]
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

