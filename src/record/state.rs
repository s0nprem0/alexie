//! Record state: runtime dirs, pid/name/region files, recorder
//! detection, shell recording IPC, notifications.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use crate::proc::{resolve_tool, run};

pub(crate) fn runtime_dir() -> PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let st = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        });
        st
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

pub(crate) fn read_pid() -> Option<i32> {
    fs::read_to_string(pid_file()).ok()?.trim().parse().ok()
}

/// Arguments selecting the recorder to act on, like legacy
/// record.sh: `--pid N` when the saved pid is still a recorder
/// (checked via record-process, which verifies /proc/<pid>/exe),
/// otherwise empty (any recorder of ours). This avoids the old
/// `kill -0 $pid` / `pgrep -f gpu-screen-recorder` checks, which
/// false-positive on a reused pid or any process mentioning the
/// recorder name in its full command line.
pub(crate) fn recorder_args() -> Vec<String> {
    if let Some(pid) = read_pid() {
        let rp = resolve_tool("record-process");
        if Command::new(&rp)
            .args(["--pid", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return vec!["--pid".to_string(), pid.to_string()];
        }
    }
    vec![]
}

pub(crate) fn is_recording() -> bool {
    let rp = resolve_tool("record-process");
    Command::new(&rp)
        .args(recorder_args())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub(crate) fn shell_recording(state: &str) {
    let _ = run(&qs(), &["ipc", "call", "capture", "recording", state]);
}

pub(crate) fn notify_msg(urgency: &str, title: &str, body: &str) {
    let _ = run(&notify(), &["-u", urgency, "-a", "record", "-G", "f03d", title, body]);
}

