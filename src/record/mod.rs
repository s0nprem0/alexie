//! Screen recording: start/stop/status/toggle.
//!
//! Split from a single 342-line record.rs:
//! - `state` — dirs, pid files, recorder detection, notify
//! - `start` — capture target resolution, spawn, chrono
//! - `stop` — signal, wait, finalise/trim
//!
//! `handle()` below is just argument parsing/dispatch.

mod state;
mod start;
mod stop;

use crate::proc::{resolve_tool, run};
use state::{cancel_file, is_recording};
use std::fs;

pub(crate) fn record_status() -> String {
    if is_recording() { "recording".to_string() } else { "stopped".to_string() }
}

pub fn handle(args: &[String]) -> String {
    let mut action = "toggle".to_string();
    let mut fullscreen = false;
    let mut audio = String::new();
    let mut webcam_size = "medium".to_string();
    let mut webcam_device = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--status" => action = "status".to_string(),
            "--stop" => action = "stop".to_string(),
            "--fullscreen" => fullscreen = true,
            s if s.starts_with("--audio=") => audio = s[8..].to_string(),
            s if s.starts_with("--webcam-device=") => webcam_device = s[16..].to_string(),
            s if s.starts_with("--webcam-size=") => webcam_size = s[14..].to_string(),
            "-h" | "--help" => return "record [--status|--stop|--fullscreen|--audio=MODE|--webcam-device=DEV|--webcam-size=SIZE]".to_string(),
            _ => { return format!("ERR unknown argument: {}", args[i]); }
        }
        i += 1;
    }

    match action.as_str() {
        "status" => record_status(),
        "stop" => {
            let _ = fs::write(cancel_file(), "");
            if is_recording() { stop::stop_recording() } else { "ERR not recording".to_string() }
        }
        "toggle" => {
            if is_recording() { return stop::stop_recording(); }
            // audio prompt if missing
            if audio.is_empty() {
                // delegate to legacy for the picker-driven flow
                let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let tool = resolve_tool("record-legacy");
                let (code, out, err) = run(&tool, &a);
                if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
                else { format!("ERR (exit {}) {}", code, err.trim()) }
            } else {
                start::start_recording(fullscreen, &audio, &webcam_size, &webcam_device, None)
            }
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let tool = resolve_tool("record-legacy");
            let (code, out, err) = run(&tool, &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_recording_or_stopped() {
        let out = handle(&["--status".to_string()]);
        assert!(matches!(out.as_str(), "recording" | "stopped"), "got: {out}");
    }

    #[test]
    fn unknown_argument_is_err() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown argument:"), "got: {out}");
    }
}
