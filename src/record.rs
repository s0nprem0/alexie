use crate::proc::{resolve_tool, run};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn runtime_dir() -> PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let st = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        });
        st
    });
    PathBuf::from(rt).join("anixie")
}

fn out_dir() -> PathBuf {
    if let Ok(v) = env::var("SCREENRECORD_DIR") { return PathBuf::from(v); }
    if let Ok(v) = env::var("XDG_VIDEOS_DIR") { return PathBuf::from(v); }
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(format!("{}/Videos", home))
}

fn gsr() -> String { env::var("GSR").unwrap_or_else(|_| "gpu-screen-recorder".to_string()) }
fn ffmpeg() -> String { env::var("FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string()) }
fn notify() -> String { env::var("NOTIFY_SEND").unwrap_or_else(|_| "notify-send".to_string()) }
fn notify_raw() -> String { env::var("NOTIFY_SEND_RAW").unwrap_or_else(|_| "notify-send".to_string()) }
fn shot_pick() -> String { env::var("SHOT_PICK").unwrap_or_else(|_| "shot-pick".to_string()) }
fn webcam_overlay() -> String { env::var("WEBCAM_OVERLAY").unwrap_or_else(|_| "webcam-overlay".to_string()) }
fn qs() -> String { env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string()) }

fn pid_file() -> PathBuf { runtime_dir().join("anixie-record.pid") }
fn name_file() -> PathBuf { runtime_dir().join("anixie-record.file") }
fn region_file() -> PathBuf { runtime_dir().join("anixie-record.region") }
fn cancel_file() -> PathBuf { runtime_dir().join("anixie-record.cancel") }

fn read_pid() -> Option<i32> {
    fs::read_to_string(pid_file()).ok()?.trim().parse().ok()
}

fn is_recording() -> bool {
    if let Some(pid) = read_pid() {
        // check process alive
        if Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    // fallback: any gsr recorder running
    let (code, _, _) = run("pgrep", &["-f", "gpu-screen-recorder"]);
    code == 0
}

fn shell_recording(state: &str) {
    let _ = run(&qs(), &["ipc", "call", "capture", "recording", state]);
}

fn notify_msg(urgency: &str, title: &str, body: &str) {
    let _ = run(&notify(), &["-u", urgency, "-a", "record", "-G", "f03d", title, body]);
}

fn record_status() -> String {
    if is_recording() { "recording".to_string() } else { "stopped".to_string() }
}

fn stop_recording() -> String {
    let _ = fs::write(cancel_file(), "");
    // find recorder pid
    let pid = read_pid();
    let mut rec_args: Vec<String> = Vec::new();
    if let Some(p) = pid {
        if Command::new(resolve_tool("record-process")).args(["--pid", &p.to_string()]).status().map(|s| s.success()).unwrap_or(false) {
            rec_args = vec!["--pid".to_string(), p.to_string()];
        }
    }
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

fn start_recording(fullscreen: bool, audio: &str, webcam_size: &str, webcam_device: &str, target: Option<String>) -> String {
    let _ = fs::remove_file(pid_file());
    let od = out_dir();
    if fs::create_dir_all(&od).is_err() {
        notify_msg("critical", "Recording failed", &format!("Could not create {}.", od.display()));
        return "ERR".to_string();
    }

    let tgt = match target {
        Some(t) => t,
        None => {
            let sp = shot_pick();
            if fullscreen {
                let (code, out, _) = run(&sp, &["fullscreen", "--match-monitor"]);
                if code != 0 { return "ERR picker failed".to_string(); }
                out.trim().to_string()
            } else {
                let (code, out, _) = run(&sp, &["smart", "--match-monitor"]);
                if code != 0 { return "ERR picker failed".to_string(); }
                out.trim().to_string()
            }
        }
    };
    if tgt.is_empty() { return "ERR no target".to_string(); }

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
            notify_msg("critical", "Recording failed", &format!("Unexpected picker result: {}", tgt));
            return "ERR".to_string();
        }
    }

    if audio == "desktop+mic+webcam" {
        let (code, _, _) = run(&webcam_overlay(), &["start", webcam_size, webcam_device, &tgt]);
        if code != 0 {
            notify_msg("critical", "Recording failed", "Could not start the webcam overlay.");
            return "ERR".to_string();
        }
    }

    let name = format!("{}/screenrecording-{}.mp4", od.display(), chrono_stamp());
    let audio_flags: Vec<String> = match audio {
        "desktop" => vec!["-a".to_string(), "default_output".to_string(), "-ac".to_string(), "aac".to_string()],
        "desktop+mic" | "desktop+mic+webcam" => vec!["-a".to_string(), "default_output|default_input".to_string(), "-ac".to_string(), "aac".to_string()],
        _ => vec![],
    };

    let mut cmd = Command::new(gsr());
    cmd.args(&capture_args);
    cmd.args(["-k", "auto", "-f", "60", "-fm", "cfr", "-fallback-cpu-encoding", "yes", "-o", &name]);
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
            notify_msg("critical", "Recording failed", &format!("Could not start recorder: {}", e));
            return "ERR".to_string();
        }
    };
    let pid = child.id();

    // wait for file to appear (up to 15s)
    let cancel = cancel_file();
    let mut ticks = 0;
    let mut ready = false;
    while ticks < 75 {
        if cancel.exists() { break; }
        if !std::path::Path::new(&name).exists() {
            // check process alive
            if !Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false) {
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
        let _ = Command::new("kill").args(["-INT", &pid.to_string()]).status();
        let _ = fs::remove_file(&name);
        let _ = run(&webcam_overlay(), &["stop"]);
        let _ = fs::remove_file(cancel_file());
        return "ERR cancelled".to_string();
    }

    if !ready {
        let _ = Command::new("kill").args(["-INT", &pid.to_string()]).status();
        let _ = fs::remove_file(&name);
        let _ = run(&webcam_overlay(), &["stop"]);
        notify_msg("critical", "Recording failed", "gpu-screen-recorder did not start.");
        return "ERR".to_string();
    }

    // process alive?
    if !Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false) {
        let _ = fs::remove_file(&name);
        let _ = run(&webcam_overlay(), &["stop"]);
        notify_msg("critical", "Recording failed", "gpu-screen-recorder exited before it started.");
        return "ERR".to_string();
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

fn chrono_stamp() -> String {
    let (code, out, _) = run("date", &["+%Y-%m-%d_%H-%M-%S"]);
    if code == 0 { out.trim().to_string() } else { "0".to_string() }
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
            if is_recording() { stop_recording() } else { "ERR not recording".to_string() }
        }
        "toggle" => {
            if is_recording() { return stop_recording(); }
            // audio prompt if missing
            if audio.is_empty() {
                // delegate to legacy for the picker-driven flow
                let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let tool = resolve_tool("record-legacy");
                let (code, out, err) = run(&tool, &a);
                if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
                else { format!("ERR (exit {}) {}", code, err.trim()) }
            } else {
                start_recording(fullscreen, &audio, &webcam_size, &webcam_device, None)
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
