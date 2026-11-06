use crate::proc::{resolve_tool, run};
use serde_json::Value;
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

fn gsr() -> String { env::var("GSR").unwrap_or_else(|_| "gpu-screen-recorder".to_string()) }
fn hyprctl() -> String { "hyprctl".to_string() }
fn mpv() -> String { env::var("MPV").unwrap_or_else(|_| "mpv".to_string()) }

const MARGIN: i64 = 40;

fn list_devices() -> String {
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

fn choose_resolution(device: &str) -> String {
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

fn overlay_address() -> String {
    let (code, out, _) = run(&hyprctl(), &["clients", "-j"]);
    if code != 0 { return String::new(); }
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    if let Some(arr) = v.as_array() {
        for c in arr {
            if let Some(cls) = c.get("class").and_then(|x| x.as_str()) {
                if cls.starts_with("WebcamOverlay-") {
                    if let Some(addr) = c.get("address").and_then(|x| x.as_str()) { return addr.to_string(); }
                }
            }
        }
    }
    String::new()
}

fn start_overlay(size: &str, device: &str, target: &str) -> String {
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

fn stop_overlay() -> String {
    let _ = Command::new("pkill").args(["-f", "wayland-app-id=WebcamOverlay"]).status();
    let _ = fs::remove_file(runtime_dir().join("anixie-record.region"));
    "OK".to_string()
}

fn resize(action: &str) -> String {
    let (code, out, _) = run(&hyprctl(), &["clients", "-j"]);
    if code != 0 { return "OK".to_string(); }
    let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    let client = v.as_array().and_then(|arr| arr.iter().find(|c| {
        c.get("class").and_then(|x| x.as_str()).map(|cls| cls.starts_with("WebcamOverlay-")).unwrap_or(false)
    })).cloned();
    let client = match client { Some(c) => c, None => return "OK".to_string() };
    let address = client.get("address").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if address.is_empty() { return "OK".to_string(); }
    let cur_w = client.get("size").and_then(|s| s.get(0)).and_then(|x| x.as_i64()).unwrap_or(0);
    let cur_h = client.get("size").and_then(|s| s.get(1)).and_then(|x| x.as_i64()).unwrap_or(0);
    let monitor_id = client.get("monitor").and_then(|x| x.as_i64()).unwrap_or(0);

    let (code, mon_out, _) = run(&hyprctl(), &["monitors", "-j"]);
    if code != 0 { return "OK".to_string(); }
    let mons: Value = serde_json::from_str(&mon_out).unwrap_or(Value::Null);
    let mon = mons.as_array().and_then(|arr| arr.iter().find(|m| m.get("id").and_then(|x| x.as_i64()) == Some(monitor_id))).cloned();
    let mon = match mon { Some(m) => m, None => return "OK".to_string() };
    let rotated = mon.get("transform").and_then(|x| x.as_i64()).unwrap_or(0) % 2 == 1;
    let mon_x = mon.get("x").and_then(|x| x.as_i64()).unwrap_or(0);
    let mon_y = mon.get("y").and_then(|x| x.as_i64()).unwrap_or(0);
    let mon_w = if rotated { mon.get("height").and_then(|x| x.as_f64()).unwrap_or(0.0) / mon.get("scale").and_then(|x| x.as_f64()).unwrap_or(1.0) } else { mon.get("width").and_then(|x| x.as_f64()).unwrap_or(0.0) / mon.get("scale").and_then(|x| x.as_f64()).unwrap_or(1.0) } as i64;
    let mon_h = if rotated { mon.get("width").and_then(|x| x.as_f64()).unwrap_or(0.0) / mon.get("scale").and_then(|x| x.as_f64()).unwrap_or(1.0) } else { mon.get("height").and_then(|x| x.as_f64()).unwrap_or(0.0) / mon.get("scale").and_then(|x| x.as_f64()).unwrap_or(1.0) } as i64;

    let mut anchor_x = mon_x;
    let mut anchor_y = mon_y;
    let mut anchor_w = mon_w;
    let mut anchor_h = mon_h;

    let region_file = runtime_dir().join("anixie-record.region");
    if let Ok(region) = fs::read_to_string(&region_file) {
        let region = region.trim();
        let parts: Vec<&str> = region.split('+').collect();
        if parts.len() == 3 {
            let wh: Vec<&str> = parts[0].split('x').collect();
            if wh.len() == 2 {
                if let (Ok(w), Ok(h), Ok(x), Ok(y)) = (wh[0].parse::<i64>(), wh[1].parse::<i64>(), parts[1].parse::<i64>(), parts[2].parse::<i64>()) {
                    anchor_w = w; anchor_h = h; anchor_x = x; anchor_y = y;
                }
            }
        }
    }

    let mut scale_h = anchor_h;
    let available_w = anchor_w - 2 * MARGIN;
    if available_w > 0 && (scale_h * 3 / 10) > available_w {
        scale_h = available_w * 10 / 3;
    }
    let small_h = (scale_h * 9 + 25) / 50;
    let small_w = (small_h * 8 + 4) / 9;
    let medium_h = (scale_h + 2) / 4;
    let medium_w = (medium_h * 8 + 4) / 9;
    let large_h = (scale_h * 27 + 40) / 80;
    let large_w = (large_h * 8 + 4) / 9;

    let mut target_w = cur_w;
    let mut target_h = cur_h;
    match action {
        "small" => { target_w = small_w; target_h = small_h; }
        "medium" | "reset" => { target_w = medium_w; target_h = medium_h; }
        "large" => { target_w = large_w; target_h = large_h; }
        "smaller" => {
            if large_w < cur_w { target_w = large_w; target_h = large_h; }
            else if medium_w < cur_w { target_w = medium_w; target_h = medium_h; }
            else if small_w < cur_w { target_w = small_w; target_h = small_h; }
        }
        "larger" => {
            if small_w > cur_w { target_w = small_w; target_h = small_h; }
            else if medium_w > cur_w { target_w = medium_w; target_h = medium_h; }
            else if large_w > cur_w { target_w = large_w; target_h = large_h; }
        }
        _ => {}
    }

    let mut target_x = anchor_x + anchor_w - target_w - MARGIN;
    let mut target_y = anchor_y + anchor_h - target_h - MARGIN;
    if target_x < anchor_x + MARGIN { target_x = anchor_x + MARGIN; }
    if target_y < anchor_y + MARGIN { target_y = anchor_y + MARGIN; }

    let window = format!("address:{}", address);
    let _ = run(&hyprctl(), &["dispatch", &format!("hl.dsp.window.resize({{ window = \"{}\", x = {}, y = {} }})", window, target_w, target_h)]);
    let _ = run(&hyprctl(), &["dispatch", &format!("hl.dsp.window.move({{ window = \"{}\", x = {}, y = {} }})", window, target_x, target_y)]);
    "OK".to_string()
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "list" => list_devices(),
        "start" => {
            let size = args.get(1).cloned().unwrap_or_else(|| "medium".to_string());
            let device = args.get(2).cloned().unwrap_or_default();
            let target = args.get(3).cloned().unwrap_or_default();
            start_overlay(&size, &device, &target)
        }
        "stop" => stop_overlay(),
        "resize" => {
            let action = args.get(1).cloned().unwrap_or_else(|| "reset".to_string());
            resize(&action)
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let tool = resolve_tool("webcam-overlay-legacy");
            let (code, out, err) = run(&tool, &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}
