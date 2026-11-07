//! Record state: runtime dirs, pid/name/region files, recorder
//! detection, shell recording IPC, notifications.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use crate::proc::run;

pub(crate) fn runtime_dir() -> PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        
        env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        })
    });
    PathBuf::from(rt).join("anixie")
}

pub(crate) fn out_dir() -> PathBuf {
    if let Ok(v) = env::var("SCREENRECORD_DIR") { return PathBuf::from(v); }
    if let Ok(v) = env::var("XDG_VIDEOS_DIR") { return PathBuf::from(v); }
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(format!("{}/Videos", home))
}

pub(crate) fn gsr() -> String { env::var("GSR").unwrap_or_else(|_| "gpu-screen-recorder".to_string()) }
pub(crate) fn ffmpeg() -> String { env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string()) }
pub(crate) fn notify() -> String { env::var("NOTIFY_SEND").unwrap_or_else(|_| "notify-send".to_string()) }
pub(crate) fn notify_raw() -> String { env::var("NOTIFY_SEND_RAW").unwrap_or_else(|_| "notify-send".to_string()) }
pub(crate) fn shot_pick() -> String { env::var("SHOT_PICK").unwrap_or_else(|_| "shot-pick".to_string()) }
pub(crate) fn webcam_overlay() -> String { env::var("WEBCAM_OVERLAY").unwrap_or_else(|_| "webcam-overlay".to_string()) }
pub(crate) fn qs() -> String { env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string()) }

pub(crate) fn pid_file() -> PathBuf { runtime_dir().join("anixie-record.pid") }
pub(crate) fn name_file() -> PathBuf { runtime_dir().join("anixie-record.file") }
pub(crate) fn region_file() -> PathBuf { runtime_dir().join("anixie-record.region") }
pub(crate) fn cancel_file() -> PathBuf { runtime_dir().join("anixie-record.cancel") }
pub(crate) fn lock_file() -> PathBuf { runtime_dir().join("anixie-record.lock") }
pub(crate) fn starting_file() -> PathBuf { runtime_dir().join("anixie-record.starting") }

pub(crate) fn read_pid() -> Option<i32> {
    fs::read_to_string(pid_file()).ok()?.trim().parse().ok()
}

fn proc_root() -> PathBuf {
    let root = env::var("RECORD_PROC_ROOT").unwrap_or_else(|_| "/proc".to_string());
    PathBuf::from(root.trim_end_matches('/'))
}

fn is_recorder(pid: i32, root: &std::path::Path) -> bool {
    // The executable is checked, not just the process name: Linux
    // truncates names to 15 chars, so a plain pgrep would match any
    // executable sharing the gpu-screen-recorder prefix, and a
    // process can rename itself besides. /proc/<pid>/exe settles it.
    let Ok(exe) = fs::read_link(root.join(pid.to_string()).join("exe")) else {
        return false;
    };
    let Some(name) = exe.file_name().map(|n| n.to_string_lossy().to_string()) else {
        return false;
    };
    let name = name.strip_suffix(" (deleted)").unwrap_or(&name);
    name == "gpu-screen-recorder"
}

fn same_user(pid: i32, root: &std::path::Path) -> bool {
    // A missing status file (a fixture tree, or a pid that vanished)
    // is not a reason to drop it.
    let status = root.join(pid.to_string()).join("status");
    let Ok(content) = fs::read_to_string(&status) else {
        return true;
    };
    let uid = content.lines().find(|l| l.starts_with("Uid:")).and_then(|l| {
        let fields: Vec<&str> = l.split_whitespace().collect();
        fields.get(2).or(fields.get(1)).map(|s| s.to_string())
    });
    match (uid, my_uid()) {
        (Some(uid), Some(me)) => uid.parse::<u32>().ok() == Some(me),
        _ => true,
    }
}

fn my_uid() -> Option<u32> {
    let (code, out, _) = run("id", &["-u"]);
    if code == 0 {
        out.trim().parse().ok()
    } else {
        None
    }
}

/// Pids of recorders owned by this user. With `wanted`, only that
/// pid (the exe is re-checked by callers before signalling, like
/// legacy, so a reused pid is never trusted blindly).
pub(crate) fn recorder_pids(wanted: Option<i32>) -> Vec<i32> {
    let root = proc_root();
    if let Some(pid) = wanted {
        if same_user(pid, &root) && is_recorder(pid, &root) {
            return vec![pid];
        }
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&root) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<i32>() else { continue };
        if pid <= 0 {
            continue;
        }
        if same_user(pid, &root) && is_recorder(pid, &root) {
            out.push(pid);
        }
    }
    out.sort_unstable();
    out
}

/// Signal recorders (INT or KILL), re-checking the exe per pid.
/// True when at least one signal was delivered.
pub(crate) fn signal_recorders(pids: &[i32], signal: &str) -> bool {
    let root = proc_root();
    let mut delivered = false;
    for pid in pids {
        if is_recorder(*pid, &root) {
            let ok = Command::new("kill")
                .args([&format!("-{}", signal), &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                delivered = true;
            }
        }
    }
    delivered
}

pub(crate) fn is_recording() -> bool {
    if let Some(pid) = read_pid()
        && !recorder_pids(Some(pid)).is_empty()
    {
        return true;
    }
    !recorder_pids(None).is_empty()
}

pub(crate) fn shell_recording(state: &str) {
    let _ = run(&qs(), &["ipc", "call", "capture", "recording", state]);
}

pub(crate) fn notify_msg(urgency: &str, title: &str, body: &str) {
    let _ = run(&notify(), &["-u", urgency, "-a", "record", "-G", "f03d", title, body]);
}

