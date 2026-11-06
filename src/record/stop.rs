//! Stop/finalise: signal the recorder, trim and announce.

use super::state::{cancel_file, ffmpeg, name_file, notify_msg, notify_raw, pid_file, recorder_args, region_file, shell_recording, webcam_overlay};
use crate::proc::{resolve_tool, run};
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub(crate) fn stop_recording() -> String {
    let _ = fs::write(cancel_file(), "");
    let rec_args = recorder_args();
    let rp = resolve_tool("record-process");
    // signal INT
    let mut args: Vec<String> = rec_args.clone();
    args.push("--signal".to_string());
    args.push("INT".to_string());
    let _ = Command::new(&rp).args(&args).status();

    // wait up to 5s
    let mut count = 0;
    while count < 50 {
        let alive = Command::new(&rp).args(&rec_args).status().map(|s| s.success()).unwrap_or(false);
        if !alive { break; }
        thread::sleep(Duration::from_millis(100));
        count += 1;
    }
    // webcam overlay stop
    let _ = run(&webcam_overlay(), &["stop"]);

    let still = Command::new(&rp).args(&rec_args).status().map(|s| s.success()).unwrap_or(false);
    if still {
        let mut ka = rec_args.clone();
        ka.push("--signal".to_string());
        ka.push("KILL".to_string());
        let _ = Command::new(&rp).args(&ka).status();
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

