//! Overlay lifecycle: device listing, resolution choice,
//! start/stop.

use super::{gsr, hyprctl, mpv, runtime_dir};
use super::resize::resize;
use crate::proc::run;
use serde_json::Value;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub(crate) fn list_devices() -> String {
    let (code, out, _) = run(&gsr(), &["--list-v4l2-devices"]);
    if code != 0 { return String::new(); }
    let mut seen = std::collections::HashSet::new();
    let mut lines = Vec::new();
    for line in out.lines() {
        let dev = line.split('|').next().unwrap_or("").trim();
        if !dev.is_empty() && seen.insert(dev.to_string()) { lines.push(dev.to_string()); }
    }
    lines.join("\n")
}

pub(crate) fn choose_resolution(device: &str) -> String {
    let (code, out, _) = run(&gsr(), &["--list-v4l2-devices"]);
    if code != 0 { return String::new(); }
    let mut formats = String::new();
    for line in out.lines() {
        let mut parts = line.split('|');
        if parts.next().unwrap_or("").trim() == device {
            formats = parts.next().unwrap_or("").to_string();
            break;
        }
    }
    for res in ["640x360", "1280x720", "1920x1080"] {
        if formats.contains(&format!("{}@", res)) { return res.to_string(); }
    }
    formats.split('@').next().unwrap_or("").to_string()
}

pub(crate) fn overlay_address() -> String {
    let (code, out, _) = run(&hyprctl(), &["clients", "-j"]);
    if code != 0 { return String::new(); }
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    if let Some(arr) = v.as_array() {
        for c in arr {
            if let Some(cls) = c.get("class").and_then(|x| x.as_str())
                && cls.starts_with("WebcamOverlay-")
                    && let Some(addr) = c.get("address").and_then(|x| x.as_str()) { return addr.to_string(); }
        }
    }
    String::new()
}

pub(crate) fn start_overlay(size: &str, device: &str, target: &str) -> String {
    if !matches!(size, "small" | "medium" | "large") {
        return format!("ERR unknown size: {} (try: small, medium, large)", size);
    }
    let mut dev = device.to_string();
    if dev.is_empty() {
        let devs = list_devices();
        dev = devs.lines().next().unwrap_or("").to_string();
    }
    if dev.is_empty() || !std::path::Path::new(&dev).exists() {
        return "ERR no camera found".to_string();
    }
    let resolution = choose_resolution(&dev);
    let mut capture_options = "framerate=30".to_string();
    if !resolution.is_empty() {
        capture_options = format!("video_size={},{}", resolution, capture_options);
    }
    let region_file = runtime_dir().join("anixie-record.region");
    if !target.is_empty() && target.starts_with("region:") {
        let _ = fs::create_dir_all(runtime_dir());
        let _ = fs::write(&region_file, &target[7..]);
    } else {
        let _ = fs::remove_file(&region_file);
    }
    let mut cmd = Command::new(mpv());
    cmd.args(["av://v4l2:".to_string() + &dev,
        "--profile=low-latency".to_string(), "--untimed".to_string(), "--no-cache".to_string(),
        format!("--demuxer-lavf-o={}", capture_options),
        "--vf=lavfi=[crop=ih*8/9:ih]".to_string(),
        "--title=WebcamOverlay".to_string(),
        format!("--wayland-app-id=WebcamOverlay-{}", size),
        "--no-border".to_string(), "--no-audio".to_string(), "--no-osc".to_string(),
        "--osd-level=0".to_string(), "--really-quiet".to_string()]);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let child = match cmd.spawn() { Ok(c) => c, Err(e) => return format!("ERR spawn mpv: {}", e) };
    let overlay_pid = child.id();
    let mut waited = 0;
    while waited < 40 && overlay_address().is_empty() {
        thread::sleep(Duration::from_millis(50));
        waited += 1;
    }
    let _ = resize(size);
    let alive = Command::new("kill").args(["-0", &overlay_pid.to_string()]).status().map(|s| s.success()).unwrap_or(false);
    if !alive && overlay_address().is_empty() {
        return format!("ERR mpv could not open {}", dev);
    }
    thread::sleep(Duration::from_millis(600));
    "OK".to_string()
}

pub(crate) fn stop_overlay() -> String {
    let _ = Command::new("pkill").args(["-f", "wayland-app-id=WebcamOverlay"]).status();
    let _ = fs::remove_file(runtime_dir().join("anixie-record.region"));
    "OK".to_string()
}

