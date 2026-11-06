//! Native clipboard watcher: pid lifecycle, `wl-paste --watch`
//! children, and the `clipboard store` child mode. Sensitive
//! (passwordManagerHint) selections are never stored.

use super::{cliphist, exe, watcher_pid_file, wl_paste};
use crate::proc::{find_in_path, run, run_bytes_with_stdin};
use std::env;
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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

pub fn ensure_watcher() {
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

