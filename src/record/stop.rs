//! Stop/finalise: signal the recorder, trim and announce.

use super::state::{cancel_file, ffmpeg, name_file, notify_msg, notify_raw, pid_file, recorder_pids, region_file, shell_recording, signal_recorders, webcam_overlay};
use crate::proc::run;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn recorder_pids_at(pids: &[i32]) -> Vec<i32> {
    pids.iter().cloned().filter(|pid| !recorder_pids(Some(*pid)).is_empty()).collect()
}

fn recorded_or_any() -> Vec<i32> {
    // The recorded pid when it is still a recorder, else any of
    // ours — the same selection legacy record.sh used.
    if let Some(pid) = super::state::read_pid()
        && !recorder_pids(Some(pid)).is_empty()
    {
        return vec![pid];
    }
    recorder_pids(None)
}

pub(crate) fn stop_recording() -> String {
    let _ = fs::write(cancel_file(), "");
    let pids = recorded_or_any();
    let _ = signal_recorders(&pids, "INT");

    // wait up to 5s
    for _ in 0..50 {
        if recorder_pids_at(&pids).is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    // webcam overlay stop
    let _ = run(&webcam_overlay(), &["stop"]);

    let still = recorder_pids_at(&pids);
    if !still.is_empty() {
        let _ = signal_recorders(&still, "KILL");
        notify_msg("critical", "Recording failed", "It did not stop cleanly.");
    } else {
        if let Ok(name) = fs::read_to_string(name_file()) {
            let name = name.trim().to_string();
            if !name.is_empty() && std::path::Path::new(&name).exists() {
                // finalize: trim first 0.1s
                let processed = format!("{}-processed.mp4", name.trim_end_matches(".mp4"));
                let _ = Command::new(ffmpeg()).args(["-y", "-ss", "0.1", "-i", &name, "-c:v", "copy", "-movflags", "+faststart", &processed]).status();
                if std::path::Path::new(&processed).exists() {
                    let _ = fs::rename(&processed, &name);
                }
                // announce
                let _ = run(&notify_raw(), &["-a", "record", "-u", "normal", "-t", "10000", "-G", "f03d", "-A", "default=Play", "Recording saved", &name]);
            }
        }
    }
    let _ = fs::remove_file(pid_file());
    let _ = fs::remove_file(name_file());
    let _ = fs::remove_file(region_file());
    let _ = fs::remove_file(cancel_file());
    shell_recording("stop");
    "OK".to_string()
}

