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

use state::{cancel_file, is_recording, lock_file, starting_file};
use std::fs;
use std::os::unix::io::AsRawFd;

pub(crate) fn record_status() -> String {
    if is_recording() { "recording".to_string() } else { "stopped".to_string() }
}

/// Ask which audio mix to record, through the shell's picker.
/// The answer does not come back to this process: the pick runs
/// `record --audio=<which>`, which starts like a keypress that
/// already knew the answer. The webcam entry only appears when a
/// camera is actually there.
fn choose_audio() -> String {
    let mut lines = vec![
        "No audio\tnone".to_string(),
        "Desktop audio\tdesktop".to_string(),
        "Desktop + microphone\tdesktop+mic".to_string(),
    ];
    if !crate::webcam_overlay::list_devices().is_empty() {
        lines.push("Desktop + microphone + webcam\tdesktop+mic+webcam".to_string());
    }
    let data = lines.join("\n") + "\n";
    crate::pick::handle_with_stdin(
        &["--title".to_string(), "record audio".to_string(), "--on-select".to_string(), "record --audio={}".to_string()],
        &data,
    )
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
            // Serialize starts: two toggles racing must not both
            // begin a recording. A second request that finds the
            // lock busy asks the start in progress to cancel rather
            // than queueing behind it (legacy flock semantics).
            let lock_path = lock_file();
            let _ = fs::create_dir_all(lock_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("/tmp")));
            let Ok(lock_file_handle) = fs::OpenOptions::new().create(true).write(true).truncate(false).open(&lock_path) else {
                return "ERR could not open the record lock".to_string();
            };
            let locked = unsafe { libc::flock(lock_file_handle.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 };
            if !locked {
                let _ = fs::write(cancel_file(), "");
                return "OK".to_string();
            }
            let _guard = lock_file_handle;

            if is_recording() { return stop::stop_recording(); }
            let _ = fs::remove_file(cancel_file());
            // audio prompt if missing
            if audio.is_empty() {
                return choose_audio();
            }
            let _ = fs::write(starting_file(), std::process::id().to_string());
            let result = start::start_recording(fullscreen, &audio, &webcam_size, &webcam_device, None);
            let _ = fs::remove_file(starting_file());
            if result.starts_with("ERR") {
                let _ = fs::remove_file(cancel_file());
            }
            result
        }
        _ => format!("ERR unknown action: {}", action),
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
