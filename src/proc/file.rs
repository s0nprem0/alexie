//! Small utilities: temp files and clipboard copy helpers.
//!
//! The copy helpers take the wl-copy binary explicitly so callers
//! keep their own env-overridable names instead of shell pipelines.

use super::tool::resolve_tool;
use std::io::Write;
use std::process::Command;

pub fn tmp_file(prefix: &str, ext: &str) -> std::path::PathBuf {
    // Callers pass literal names today, but keep user input from
    // escaping the temp dir if a future caller is careless.
    assert!(!prefix.contains('/') && !prefix.contains('\\'),
        "tmp_file prefix must not contain path separators");
    assert!(!ext.contains('/') && !ext.contains('\\'),
        "tmp_file ext must not contain path separators");
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
        format!(
            "anixie-{}-{}-{}-{}.{}",
            prefix,
            std::process::id(),
            nanos,
            n,
            ext
        )
    };
    std::env::temp_dir().join(name)
}

/// Copy `data` to the clipboard via wl-copy, without a shell.
/// `extra` carries flags like `--sensitive`.
pub fn copy_text_to_clipboard(wl_bin: &str, mime: &str, data: &str, extra: &[&str]) -> i32 {
    use std::process::Stdio;
    let mut cmd = Command::new(wl_bin);
    cmd.args(extra);
    cmd.args(["--type", mime]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return -1,
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(data.as_bytes()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return -1;
        }
    }
    child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1)
}

/// Copy a file's bytes to the clipboard via wl-copy, binary-safe
/// and without shell redirection (`wl-copy --type T < file`).
pub fn copy_file_to_clipboard(wl_bin: &str, mime: &str, path: &std::path::Path) -> i32 {
    use std::process::Stdio;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return -1,
    };
    Command::new(wl_bin)
        .args(["--type", mime])
        .stdin(Stdio::from(file))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.code().unwrap_or(-1))
        .unwrap_or(-1)
}

/// Run `cmd args`, writing its (binary) stdout to `dest`.
/// Used for `cliphist decode`, where `run()`'s lossy UTF-8
/// conversion would corrupt image data and shell redirection
/// (`cmd decode ID > path`) left `id`/`path` unquoted.
pub fn run_stdout_to_file(cmd: &str, args: &[&str], dest: &std::path::Path) -> (i32, String) {
    let file = match std::fs::File::create(dest) {
        Ok(f) => f,
        Err(e) => return (-1, e.to_string()),
    };
    match Command::new(resolve_tool(cmd))
        .args(args)
        .stdout(std::process::Stdio::from(file))
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(o) => (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stderr).to_string(),
        ),
        Err(e) => (-1, e.to_string()),
    }
}
