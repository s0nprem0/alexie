use std::process::Command;

/// Find an executable by name in a PATH-style string, without
/// spawning a shell. Pure, so it can be tested without mutating
/// the process PATH (which races other tests mutating PATH).
pub fn find_in_path(name: &str, path_str: &str) -> Option<std::path::PathBuf> {
    // Names containing a path separator are used as-is: if the file
    // exists, that is the binary; otherwise Command::new will fail.
    if name.contains('/') {
        let p = std::path::PathBuf::from(name);
        return if is_executable(&p) { Some(p) } else { None };
    }
    for dir in path_str.split(':') {
        if dir.is_empty() {
            continue;
        }
        let p = std::path::Path::new(dir).join(name);
        if is_executable(&p) {
            return Some(p);
        }
    }
    None
}

fn is_executable(p: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

pub fn resolve_tool(name: &str) -> String {
    // Prefer `name-legacy` so delegation never recurses through an
    // alexie shim, falling back to `name` itself. The old version
    // spawned `sh -c "command -v …"` twice on *every* external call
    // (hyprctl, date, jq, …), which dominates the cost of short
    // commands like status queries.
    let path = std::env::var("PATH").unwrap_or_default();
    let legacy = format!("{}-legacy", name);
    if find_in_path(&legacy, &path).is_some() {
        return legacy;
    }
    if find_in_path(name, &path).is_some() {
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

/// Copy `data` to the clipboard via wl-copy, without a shell.
/// `extra` carries flags like `--sensitive`.
pub fn copy_text_to_clipboard(wl_bin: &str, mime: &str, data: &str, extra: &[&str]) -> i32 {
    use std::io::Write;
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
        let _ = stdin.write_all(data.as_bytes());
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
    fn copy_text_and_file_to_clipboard() {
        // A fake wl-copy that ignores --type and discards stdin,
        // proving both helpers feed stdin without a shell. (`cat`
        // itself rejects --type, so it cannot stand in here.)
        let dir0 = std::env::temp_dir().join(format!(
            "alexie-copy-wl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir0).unwrap();
        let wl = dir0.join("wl-copy");
        std::fs::write(&wl, "#!/bin/sh\ncat >/dev/null\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wl, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let wl = wl.to_string_lossy().to_string();
        // Retry a spawn failure: under full-suite parallelism the
        // process table can transiently refuse a fork (EMFILE), which
        // surfaces as -1 from the helper. Production callers treat
        // -1 as "clipboard unavailable" and fall back to saving.
        let mut code = -1;
        for _ in 0..5 {
            code = copy_text_to_clipboard(&wl, "text/plain", "hello", &[]);
            if code == 0 { break; }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(code, 0);
        let _ = std::fs::remove_dir_all(&dir0);
        let dir = std::env::temp_dir().join(format!(
            "alexie-copy-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("data.bin");
        std::fs::write(&f, b"\x89PNG\r\n\x1a\n").unwrap();
        // recreate the fake wl-copy (dir0 was the bindir; rebuild
        // a fresh one here for the file-copy case)
        let dir1 = dir.join("bin");
        std::fs::create_dir_all(&dir1).unwrap();
        let wl1 = dir1.join("wl-copy");
        std::fs::write(&wl1, "#!/bin/sh\ncat >/dev/null\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wl1, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut code = -1;
        for _ in 0..5 {
            code = copy_file_to_clipboard(&wl1.to_string_lossy(), "image/png", &f);
            if code == 0 { break; }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(code, 0);
        let (code, _err) = run_stdout_to_file("cat", &[&f.to_string_lossy().to_string()], &dir.join("out.bin"));
        assert_eq!(code, 0);
        assert_eq!(std::fs::read(dir.join("out.bin")).unwrap(), b"\x89PNG\r\n\x1a\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tmp_file_unique_per_call() {
        let a = tmp_file("test", "png");
        let b = tmp_file("test", "png");
        assert_ne!(a, b);
        assert!(a.to_string_lossy().ends_with(".png"));
        assert!(a.to_string_lossy().contains("anixie-test-"));
    }

    #[test]
    fn find_in_path_prefers_executable() {
        let dir = std::env::temp_dir().join(format!(
            "alexie-find-in-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("mytool");
        std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = dir.to_string_lossy().to_string();
        assert!(find_in_path("mytool", &path).is_some());
        assert!(find_in_path("no-such-tool-xyz", &path).is_none());
        // A non-executable file does not count.
        let plain = dir.join("plain");
        std::fs::write(&plain, "hi").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        assert!(find_in_path("plain", &path).is_none());
        // Absolute paths are used as-is.
        assert!(find_in_path(&bin.to_string_lossy(), "/nonexistent").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
