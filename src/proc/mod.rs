//! Process execution and tool resolution.
//!
//! Split from a single 369-line proc.rs:
//! - `tool` — PATH search (find_in_path), legacy-aware resolve_tool
//! - `run` — run/run_with_stdin/run_bytes, shell helpers
//! - `util` — tmp_file, clipboard copy, stdout-to-file
//!
//! All public names are re-exported here so `crate::proc::*`
//! callers are unchanged.

mod exec;
mod file;
mod tool;

pub use exec::{run, run_bytes_with_stdin, run_with_stdin, shell_quote};
pub use file::{copy_file_to_clipboard, copy_text_to_clipboard, run_stdout_to_file, tmp_file};
pub use tool::{find_in_path, notify_send, quickshell, resolve_tool, tool, wl_copy};

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
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
            if code == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(code, 0);
        let _ = std::fs::remove_dir_all(&dir0);
        let dir = std::env::temp_dir().join(format!(
            "alexie-copy-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
            if code == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(code, 0);
        let f_str = f.to_string_lossy();
        let (code, _err) = run_stdout_to_file("cat", &[f_str.as_ref()], &dir.join("out.bin"));
        assert_eq!(code, 0);
        assert_eq!(
            std::fs::read(dir.join("out.bin")).unwrap(),
            b"\x89PNG\r\n\x1a\n"
        );
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
