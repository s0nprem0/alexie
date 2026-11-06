use crate::proc::{find_in_path, run, run_bytes_with_stdin, run_stdout_to_file, run_with_stdin};
use std::env;
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
fn cliphist() -> String { env::var("CLIPHIST").unwrap_or_else(|_| "cliphist".to_string()) }
fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }
fn wl_paste() -> String { env::var("WL_PASTE").unwrap_or_else(|_| "wl-paste".to_string()) }
fn file_bin() -> String { env::var("FILE").unwrap_or_else(|_| "file".to_string()) }
fn qs() -> String { env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string()) }

fn exe() -> String {
    env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "alexie".to_string())
}

fn runtime_dir() -> std::path::PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(rt)
}

fn preview_dir() -> std::path::PathBuf {
    // XDG_RUNTIME_DIR is already per-user (tmpfs, 0700), so the old
    // `anixie-clipboard-{uid}` suffix via a shell-out to `id -u`
    // added nothing. Keep a fixed name; old uid-suffixed previews
    // are orphaned and recreated on demand.
    runtime_dir().join("anixie-clipboard")
}

fn watcher_pid_file() -> std::path::PathBuf {
    runtime_dir().join("clipboard-watch.pid")
}

fn pid_alive(pid: i64) -> bool {
    // kill -0 is the portable alive check the legacy script uses.
    let (code, _, _) = run("kill", &["-0", &pid.to_string()]);
    code == 0
}

fn watcher_pid_alive() -> bool {
    if let Ok(s) = std::fs::read_to_string(watcher_pid_file()) {
        if let Ok(pid) = s.trim().parse::<i64>() {
            return pid_alive(pid);
        }
    }
    false
}

// The daemon is threaded (one thread per connection) and this runs at
// the top of every clipboard call. Without a lock, two concurrent
// calls can both see a missing/dead pid file and spawn two watchers
// before either child writes the pid file; the legacy guard cannot
// dedup watchers that both passed its check.
static WATCHER_LOCK: Mutex<()> = Mutex::new(());

fn ensure_watcher() {
    // Under `cargo test` this binary is the test harness; spawning
    // it as a watcher would re-run tests. Unit tests exercise
    // handle() with fakes and never need a real watcher.
    if cfg!(test) {
        return;
    }
    let _guard = WATCHER_LOCK.lock().unwrap();
    // If the watcher recorded in the pid file is still
    // alive, do nothing. This is more precise than
    // `pgrep -f clipboard-watch`, whose full-command-line match can
    // hit the pgrep process itself or unrelated shells mentioning
    // the name.
    if watcher_pid_alive() {
        return;
    }
    // No wl-paste, no watcher (avoids a pointless spawn + ~1s wait
    // on every clipboard call when the tool is missing).
    let path = env::var("PATH").unwrap_or_default();
    if find_in_path(&wl_paste(), &path).is_none() {
        return;
    }
    // This binary is the watcher; no separate clipboard-watch
    // binary to look up on PATH.
    let _ = std::process::Command::new(exe())
        .args(["clipboard", "watch"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    // The child records itself in the pid file as it starts. Wait
    // (briefly) for that, still holding the lock, so the next caller
    // sees the live pid instead of spawning a second watcher racing
    // the first. If the pid never appears (the watcher died on
    // startup), the lock is released after ~1s and the next call
    // retries.
    for _ in 0..100 {
        if watcher_pid_alive() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Whether the current selection is marked sensitive by a password
/// manager. cliphist has no way to know where a selection came from,
/// so the offer's MIME types are the only signal — and storing a
/// secret would put it in the history and every preview.
fn is_sensitive() -> bool {
    let (code, out, _) = run(&wl_paste(), &["--list-types"]);
    code == 0 && out.contains("passwordManagerHint")
}

/// Hidden child mode, run by `wl-paste --watch` for every new
/// selection: read the selection on stdin and hand it to cliphist,
/// unless it is sensitive (then drop it on the floor).
pub fn store() {
    let mut data = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut data);
    if is_sensitive() {
        return;
    }
    let _ = run_bytes_with_stdin(&cliphist(), &["store"], &data);
}

/// Atomically claim the pid file (create_new): exactly one of two
/// simultaneous starts wins; the loser sees a live pid and bails.
/// A stale record (dead pid) is removed and reclaimed. Returns
/// false when another watcher is already running.
fn claim_pid_file() -> bool {
    use std::fs::OpenOptions;
    use std::io::Write;
    for _ in 0..10 {
        match OpenOptions::new().write(true).create_new(true).open(watcher_pid_file()) {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", std::process::id());
                return true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Empty/unparsable means a starter just claimed it
                // and has not written yet — give it a moment before
                // treating the record as stale.
                if watcher_pid_alive() {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                if watcher_pid_alive() {
                    return false;
                }
                let _ = std::fs::remove_file(watcher_pid_file());
            }
            Err(_) => return false,
        }
    }
    false
}

fn release_pid_file() {
    if let Ok(s) = std::fs::read_to_string(watcher_pid_file()) {
        if s.trim() == std::process::id().to_string() {
            let _ = std::fs::remove_file(watcher_pid_file());
        }
    }
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop(_sig: i32) {
    STOP.store(true, Ordering::SeqCst);
}

/// Blocking watcher: two `wl-paste --watch` children (text and
/// image), each re-invoking this binary as `clipboard store` per
/// selection. SIGTERM/SIGINT stops and reaps both children so no
/// orphaned watcher pair outlives the parent.
pub fn watch() {
    if !claim_pid_file() {
        return;
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_stop as usize);
        libc::signal(libc::SIGINT, on_stop as usize);
    }
    let me = exe();
    let mut children: Vec<std::process::Child> = Vec::new();
    for ty in ["text", "image"] {
        match std::process::Command::new(wl_paste())
            .args(["--type", ty, "--watch", &me, "clipboard", "store"])
            .stdin(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => children.push(c),
            Err(e) => eprintln!("clipboard watch: cannot start wl-paste ({})", e),
        }
    }
    if children.is_empty() {
        release_pid_file();
        return;
    }
    while !STOP.load(Ordering::SeqCst) {
        children.retain_mut(|c| match c.try_wait() {
            Ok(Some(_)) => false,
            _ => true,
        });
        if children.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for c in &mut children {
        let _ = c.kill();
        let _ = c.wait();
    }
    release_pid_file();
}

fn cliphist_list() -> (i32, String, String) {
    run(&cliphist(), &["list"])
}

fn cliphist_delete_line(line: &str) -> (i32, String, String) {
    // cliphist delete reads the full list line on stdin. Feeding it
    // via run_with_stdin avoids `printf … | cliphist delete` shell
    // quoting for lines containing quotes or $.
    run_with_stdin(&cliphist(), &["delete"], &format!("{}\n", line))
}

fn list_ids(list: &str) -> Vec<(String, String)> {
    list.lines()
        .filter_map(|line| {
            let tab = line.find('\t')?;
            Some((line[..tab].to_string(), line.to_string()))
        })
        .collect()
}

pub fn handle(args: &[String]) -> String {
    ensure_watcher();
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        // Blocking/local-only entry points: main() dispatches these
        // before the daemon client, so reaching them here means
        // someone called via the socket. Like screentime watch,
        // point at the direct invocation.
        "watch" => "Run `alexie clipboard watch` directly (it blocks).".to_string(),
        "store" => {
            store();
            "OK".to_string()
        }
        "--copy" => {
            if args.len() < 2 { return "ERR usage: clipboard --copy ID".to_string(); }
            let id = &args[1];
            // Decode binary-safely to a temp file (no shell redirect,
            // no lossy UTF-8 conversion of image data).
            let tmp = crate::proc::tmp_file("clip", "");
            let (code, err) = run_stdout_to_file(&cliphist(), &["decode", id], &tmp);
            if code != 0 { let _ = std::fs::remove_file(&tmp); return format!("ERR {}", err.trim()); }
            if std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) == 0 {
                let _ = std::fs::remove_file(&tmp);
                return format!("ERR entry {} is empty", id);
            }
            // Snapshot existing history ids so we can drop the duplicate
            // that copying re-creates via the clipboard watcher.
            let (code, before, _) = cliphist_list();
            let mut before_ids = std::collections::HashSet::new();
            if code == 0 {
                for (bid, _) in list_ids(&before) {
                    before_ids.insert(bid);
                }
            }
            // mime
            let tmp_str = tmp.to_string_lossy().to_string();
            let (code, mime, _) = run(&file_bin(), &["-b", "--mime-type", "--", &tmp_str]);
            let mime = if code == 0 { mime.trim().to_string() } else { "application/octet-stream".to_string() };
            // copy, binary-safe, no shell redirection
            let _ = crate::proc::copy_file_to_clipboard(&wl_copy(), &mime, &tmp);
            let _ = std::fs::remove_file(&tmp);
            // Poll for the watcher-created duplicate instead of a
            // fixed sleep(300ms) followed by deleting *every* new
            // entry: a slower watcher left the duplicate behind, and
            // a user copy inside the window was deleted as if it were
            // ours. Poll up to ~2s, delete only the first new entry.
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let (code, after, _) = cliphist_list();
                if code != 0 { break; }
                let mut found = false;
                for (aid, line) in list_ids(&after) {
                    if before_ids.contains(&aid) { continue; }
                    let _ = cliphist_delete_line(&line);
                    found = true;
                    break;
                }
                if found { break; }
            }
            "OK".to_string()
        }
        "--list" => {
            let (code, out, err) = cliphist_list();
            if code != 0 { format!("ERR {}", err.trim()) } else { out.trim_end().to_string() }
        }
        "--text" => {
            if args.len() < 2 { return "ERR usage: clipboard --text ID".to_string(); }
            let (code, out, err) = run(&cliphist(), &["decode", &args[1]]);
            if code != 0 { format!("ERR {}", err.trim()) } else { out }
        }
        "--images" => {
            let dir = preview_dir().join("thumbs");
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            // chmod 700
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
            }
            let (code, list, _) = cliphist_list();
            if code != 0 { return "ERR cliphist list failed".to_string(); }
            let mut out = String::new();
            let mut count = 0;
            for line in list.lines() {
                if count >= 16 { break; }
                let tab = match line.find('\t') { Some(t) => t, None => continue };
                let id = &line[..tab];
                let preview = &line[tab+1..];
                if !preview.starts_with("[[ binary data ") { continue; }
                let path = dir.join(id);
                let (dcode, _) = run_stdout_to_file(&cliphist(), &["decode", id], &path);
                if dcode == 0 {
                    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 0 {
                        out.push_str(&format!("{}\t{}\n", id, path.display()));
                        count += 1;
                    } else {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            out.trim_end().to_string()
        }
        "--image" => {
            if args.len() < 2 { return "ERR usage: clipboard --image ID".to_string(); }
            let dir = preview_dir().join("previews");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("preview-{}", args[1]));
            let (code, err) = run_stdout_to_file(&cliphist(), &["decode", &args[1]], &path);
            if code != 0 { let _ = std::fs::remove_file(&path); return format!("ERR {}", err.trim()); }
            if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) == 0 {
                let _ = std::fs::remove_file(&path);
                return "ERR empty".to_string();
            }
            path.to_string_lossy().to_string()
        }
        "--delete" => {
            if args.len() < 2 { return "ERR usage: clipboard --delete ID".to_string(); }
            let id = &args[1];
            let (code, list, _) = cliphist_list();
            if code != 0 { return "ERR cliphist list failed".to_string(); }
            let found_line = list_ids(&list)
                .into_iter()
                .find(|(lid, _)| lid == id)
                .map(|(_, line)| line)
                .unwrap_or_default();
            if found_line.is_empty() { return "OK".to_string(); }
            let (code, _, err) = cliphist_delete_line(&found_line);
            if code != 0 { format!("ERR {}", err.trim()) } else { "OK".to_string() }
        }
        "--wipe" => {
            let (code, _, err) = run(&cliphist(), &["wipe"]);
            if code != 0 { format!("ERR {}", err.trim()) } else { "OK".to_string() }
        }
        _ => {
            // open popup via quickshell (default behavior)
            let (code, out, err) = run(&qs(), &["ipc", "call", "clipboard", "open"]);
            if code != 0 { format!("ERR (exit {}) {}", code, err.trim()) }
            else {
                let r = out.trim();
                match r {
                    "ok" | "\"ok\"" => "OK".to_string(),
                    _ => if r.is_empty() { "OK".to_string() } else { r.to_string() }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    // Env-mutating tests in this module race with each other if run
    // in parallel (Rust runs tests on threads by default). Serialize
    // them on a shared lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_fake(dir: &str, name: &str, body: &str) {
        let path = format!("{}/{}", dir, name);
        fs::write(&path, body).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    #[test]
    fn list_ids_parses_tab_separated() {
        let ids = list_ids("1\thello\n2\tworld\nno-tab-line\n");
        assert_eq!(ids, vec![
            ("1".to_string(), "1\thello".to_string()),
            ("2".to_string(), "2\tworld".to_string()),
        ]);
    }

    #[test]
    fn list_returns_cliphist_output() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = "/tmp/opencode/clip-test-bin";
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
        make_fake(dir, "cliphist", "#!/bin/sh\ncase \"$1\" in list) printf '1\\thello\\n2\\tworld\\n';; esac\n");
        make_fake(dir, "wl-copy", "#!/bin/sh\ncat >/dev/null\n");
        make_fake(dir, "file", "#!/bin/sh\nprintf 'text/plain\\n'\n");
        make_fake(dir, "quickshell", "#!/bin/sh\nprintf 'ok\\n'\n");

        std::env::set_var("CLIPHIST", format!("{}/cliphist", dir));
        std::env::set_var("WL_COPY", format!("{}/wl-copy", dir));
        std::env::set_var("FILE", format!("{}/file", dir));
        std::env::set_var("QUICKSHELL", format!("{}/quickshell", dir));
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir, old_path));

        let out = handle(&["--list".to_string()]);
        assert!(out.contains("hello"), "got: {}", out);
        assert!(out.contains("world"), "got: {}", out);
    }

    #[test]
    fn delete_uses_stdin_not_shell() {
        let _guard = ENV_LOCK.lock().unwrap();
        // cliphist delete reads the full line on stdin; a line with
        // shell metacharacters must arrive intact, not be executed.
        let dir = "/tmp/opencode/clip-delete-test";
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
        let log = format!("{}/delete-stdin.log", dir);
        make_fake(dir, "cliphist", &format!(
            "#!/bin/sh\ncase \"$1\" in list) printf '9\\t$(touch {}/pwned)\\n';; delete) cat > {};; esac\n",
            dir, log
        ));
        std::env::set_var("CLIPHIST", format!("{}/cliphist", dir));
        let out = handle(&["--delete".to_string(), "9".to_string()]);
        assert_eq!(out, "OK", "got: {out}");
        assert!(!std::path::Path::new(&format!("{}/pwned", dir)).exists(), "shell injection via delete line");
        let fed = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(fed.contains("9\t"), "got: {fed}");
        let _ = fs::remove_dir_all(dir);
    }
}
