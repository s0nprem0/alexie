//! Start: resolve the capture target, spawn gpu-screen-recorder.

use super::state::{
    cancel_file, gsr, name_file, notify_msg, out_dir, pid_file, region_file, shell_recording,
    shot_pick, webcam_overlay,
};
use crate::proc::run;
use std::env;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub(crate) fn start_recording(
    fullscreen: bool,
    audio: &str,
    webcam_size: &str,
    webcam_device: &str,
    target: Option<String>,
) -> String {
    let _ = fs::remove_file(pid_file());
    let od = out_dir();
    if fs::create_dir_all(&od).is_err() {
        notify_msg(
            "critical",
            "Recording failed",
            &format!("Could not create {}.", od.display()),
        );
        return format!("ERR could not create {}", od.display());
    }

    let tgt = match target {
        Some(t) => t,
        None => {
            let sp = shot_pick();
            if fullscreen {
                let (code, out, _) = run(&sp, &["fullscreen", "--match-monitor"]);
                if code != 0 {
                    return "ERR picker failed".to_string();
                }
                out.trim().to_string()
            } else {
                let (code, out, _) = run(&sp, &["smart", "--match-monitor"]);
                if code != 0 {
                    return "ERR picker failed".to_string();
                }
                out.trim().to_string()
            }
        }
    };
    if tgt.is_empty() {
        return "ERR no target".to_string();
    }

    let mut capture_args: Vec<String> = Vec::new();
    match tgt.as_str() {
        t if t.starts_with("monitor:") => {
            capture_args.push("-w".to_string());
            capture_args.push(t[8..].to_string());
            capture_args.push("-s".to_string());
            capture_args.push("0x0".to_string());
        }
        t if t.starts_with("region:") => {
            capture_args.push("-w".to_string());
            capture_args.push("region".to_string());
            capture_args.push("-region".to_string());
            capture_args.push(t[7..].to_string());
        }
        _ => {
            notify_msg(
                "critical",
                "Recording failed",
                &format!("Unexpected picker result: {}", tgt),
            );
            return format!("ERR unexpected picker result: {}", tgt);
        }
    }

    if audio == "desktop+mic+webcam" {
        let (code, _, _) = run(
            &webcam_overlay(),
            &["start", webcam_size, webcam_device, &tgt],
        );
        if code != 0 {
            notify_msg(
                "critical",
                "Recording failed",
                "Could not start the webcam overlay.",
            );
            return "ERR could not start the webcam overlay".to_string();
        }
    }

    let name = format!("{}/screenrecording-{}.mp4", od.display(), chrono_stamp());
    let audio_flags: Vec<String> = match audio {
        "desktop" => vec![
            "-a".to_string(),
            "default_output".to_string(),
            "-ac".to_string(),
            "aac".to_string(),
        ],
        "desktop+mic" | "desktop+mic+webcam" => vec![
            "-a".to_string(),
            "default_output|default_input".to_string(),
            "-ac".to_string(),
            "aac".to_string(),
        ],
        _ => vec![],
    };

    let mut cmd = Command::new(gsr());
    cmd.args(&capture_args);
    cmd.args([
        "-k",
        "auto",
        "-f",
        "60",
        "-fm",
        "cfr",
        "-fallback-cpu-encoding",
        "yes",
        "-o",
        &name,
    ]);
    cmd.args(&audio_flags);
    cmd.stdin(std::process::Stdio::null());
    // redirect stderr to log
    let log = env::var("SCREENRECORD_LOG").unwrap_or_else(|_| "/dev/null".to_string());
    if let Ok(f) = fs::OpenOptions::new().create(true).append(true).open(&log) {
        cmd.stderr(f);
    }
    cmd.stdout(std::process::Stdio::null());
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            notify_msg(
                "critical",
                "Recording failed",
                &format!("Could not start recorder: {}", e),
            );
            return format!("ERR could not start recorder: {}", e);
        }
    };
    let pid = child.id();

    // wait for file to appear (up to 15s)
    let cancel = cancel_file();
    let mut ticks = 0;
    let mut ready = false;
    while ticks < 75 {
        if cancel.exists() {
            break;
        }
        if !std::path::Path::new(&name).exists() {
            // check process alive
            if !Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                break;
            }
            thread::sleep(Duration::from_millis(200));
            ticks += 1;
            continue;
        }
        ready = true;
        break;
    }

    if cancel.exists() {
        let _ = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
        let _ = fs::remove_file(&name);
        let _ = run(&webcam_overlay(), &["stop"]);
        let _ = fs::remove_file(cancel_file());
        return "ERR cancelled".to_string();
    }

    if !ready {
        let _ = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
        let _ = fs::remove_file(&name);
        let _ = run(&webcam_overlay(), &["stop"]);
        notify_msg(
            "critical",
            "Recording failed",
            "gpu-screen-recorder did not start.",
        );
        return "ERR gpu-screen-recorder did not start".to_string();
    }

    // process alive?
    if !Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let _ = fs::remove_file(&name);
        let _ = run(&webcam_overlay(), &["stop"]);
        notify_msg(
            "critical",
            "Recording failed",
            "gpu-screen-recorder exited before it started.",
        );
        return "ERR gpu-screen-recorder exited before it started".to_string();
    }

    let _ = fs::write(pid_file(), pid.to_string());
    let _ = fs::write(name_file(), &name);
    // region
    if let Some(rest) = tgt.strip_prefix("region:") {
        let _ = fs::write(region_file(), rest);
    }
    shell_recording("start");
    "OK".to_string()
}

pub(crate) fn chrono_stamp() -> String {
    let (code, out, _) = run("date", &["+%Y-%m-%d_%H-%M-%S"]);
    if code == 0 {
        out.trim().to_string()
    } else {
        "0".to_string()
    }
}
