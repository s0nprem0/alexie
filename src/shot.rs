use crate::proc::{resolve_tool, run, shell_quote};
use std::env;
use std::fs;
use std::process::Command;


fn delegate(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, out, err) = run("shot-legacy", &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

fn shot_pick() -> String { env::var("SHOT_PICK").unwrap_or_else(|_| "shot-pick".to_string()) }
fn grim() -> String { env::var("GRIM").unwrap_or_else(|_| "grim".to_string()) }
fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }
fn notify() -> String { env::var("NOTIFY_SEND").unwrap_or_else(|_| "notify-send".to_string()) }
fn hyprctl() -> String { "hyprctl".to_string() }
fn jq() -> String { "jq".to_string() }

fn state_dir() -> std::path::PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    std::path::PathBuf::from(state).join("anixie")
}

fn runtime_dir() -> std::path::PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let st = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        });
        st
    });
    std::path::PathBuf::from(rt).join("anixie")
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        // default mode smart, dest preview
        return handle_mode_dest("smart", "preview", &[]);
    }
    // parse args
    let mut mode = "smart".to_string();
    let mut dest = "preview".to_string();
    let mut post: Vec<String> = Vec::new();
    let mut i = 0;
    let mut after_dash = false;
    let mut rest: Vec<String> = Vec::new();
    while i < args.len() {
        let a = args[i].as_str();
        if after_dash { post.push(a.to_string()); i += 1; continue; }
        match a {
            "smart" | "region" | "windows" | "fullscreen" | "window" | "scroll"
            | "cancel" | "edit-last" | "copy-path" | "copy-uri" => { mode = a.to_string(); }
            "save" | "copy" | "annotate" | "preview" | "redact" | "text" => { dest = a.to_string(); }
            "cancel" | "edit-last" | "copy-path" | "copy-uri" => {
                // delegate whole command
                return delegate(args);
            }
            "--" => { after_dash = true; }
            _ => { rest.push(a.to_string()); }
        }
        i += 1;
    }

    // delegate the most complex modes
    if mode == "smart" || mode == "scroll" {
        return delegate(args);
    }

    // cancel: dismiss preview, stop scrolling capture
    if mode == "cancel" {
        let runtime = runtime_dir();
        let _ = std::fs::create_dir_all(&runtime);
        let _ = std::fs::write(runtime.join("shot-scroll-cancel"), "");
        let _ = run("quickshell", &["ipc", "call", "capture", "hide"]);
        return "OK".to_string();
    }

    // edit-last: open last capture in satty
    if mode == "edit-last" {
        let last_file = runtime_dir().join("last-capture");
        let last = std::fs::read_to_string(&last_file).unwrap_or_default().trim().to_string();
        if last.is_empty() || !std::path::Path::new(&last).exists() {
            return "ERR no previous capture".to_string();
        }
        let satty = env::var("SATTY").unwrap_or_else(|_| "satty".to_string());
        match Command::new(&satty).args(["--filename", &last, "--output-filename", &last, "--initial-tool", "crop"]).spawn() {
            Ok(_) => return "OK".to_string(),
            Err(e) => return format!("ERR spawn satty: {}", e),
        }
    }

    // copy-path
    if mode == "copy-path" {
        let last_file = runtime_dir().join("last-capture");
        let last = std::fs::read_to_string(&last_file).unwrap_or_default().trim().to_string();
        if last.is_empty() || !std::path::Path::new(&last).exists() {
            return "ERR no previous capture".to_string();
        }
        let _ = run("sh", &["-c", &format!("printf '%s' {} | {} --type text/plain", shell_quote(&last), wl_copy())]);
        let _ = run("notify-send", &["-a", "capture", "-u", "low", "-t", "2000", "Copied path", &last]);
        return last;
    }

    // copy-uri
    if mode == "copy-uri" {
        let last_file = runtime_dir().join("last-capture");
        let last = std::fs::read_to_string(&last_file).unwrap_or_default().trim().to_string();
        if last.is_empty() || !std::path::Path::new(&last).exists() {
            return "ERR no previous capture".to_string();
        }
        let uri = format!("file://{}", last);
        let _ = run("sh", &["-c", &format!("printf '%s' {} | {} --type text/uri-list", shell_quote(&uri), wl_copy())]);
        let _ = run("notify-send", &["-a", "capture", "-u", "low", "-t", "2000", "Copied file URI", &uri]);
        return uri;
    }

    // redact/text/annotate destinations: open satty with specific tool
    if dest == "redact" || dest == "text" || dest == "annotate" {
        // delegate to bash shot for these (satty hand-off with specific tool)
        return delegate(args);
    }
    if !post.is_empty() {
        return delegate(args);
    }

    handle_mode_dest(&mode, &dest, &rest)
}


fn move_file(src: &str, dst: &str) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dst)?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

fn handle_mode_dest(mode: &str, dest: &str, _rest: &[String]) -> String {
    // get geometry
    let geometry = if mode == "window" {
        let (code, out, _) = run(&hyprctl(), &["activewindow", "-j"]);
        if code != 0 { return "ERR no focused window".to_string(); }
        // parse at/size
        // use jq
        let filter = r#"if .at == null or .size == null then empty else "\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])" end"#;
        let script = format!("printf '%s' {} | jq -r '{}'", shell_quote(&out), filter);
        let (code, g, _) = run("sh", &["-c", &script]);
        if code != 0 || g.trim().is_empty() { let _ = run(&notify(), &["-a", "capture", "-u", "normal", "-t", "6000", "Screenshot failed", "No focused window."]); return "ERR no focused window to capture".to_string(); }
        g.trim().to_string()
    } else if mode == "fullscreen" {
        // focused monitor geometry: 0,0 WxH
        let (code, out, _) = run(&hyprctl(), &["monitors", "-j"]);
        if code != 0 { return "ERR no monitors".to_string(); }
        let filter = r#"[.[] | select(.focused == true)][0] | "\(.x),\(.y) \(.width)x\(.height)""#;
        let script = format!("printf '%s' {} | jq -r '{}'", shell_quote(&out), filter);
        let (code, g, _) = run("sh", &["-c", &script]);
        if code != 0 || g.trim().is_empty() { let _ = run(&notify(), &["-a", "capture", "-u", "normal", "-t", "6000", "Screenshot failed", "No focused monitor."]); return "ERR no focused monitor".to_string(); }
        g.trim().to_string()
    } else {
        // region / windows: use shot-pick
        let picker_mode = if mode == "windows" { "windows" } else { "region" };
        let sp = shot_pick();
        let (code, g, err) = run(&sp, &[picker_mode, "--keep-freeze"]);
        if code != 0 { let _ = run(&notify(), &["-a", "capture", "-u", "normal", "-t", "6000", "Screenshot failed", &format!("Could not pick region: {}", err.trim())]); return format!("ERR pick: {}", err.trim()); }
        let g = g.trim().to_string();
        if g.is_empty() { let _ = run(&notify(), &["-a", "capture", "-u", "normal", "-t", "6000", "Screenshot failed", "No region selected."]); return "ERR no region".to_string(); }
        g
    };

    if geometry.is_empty() { return "ERR no geometry".to_string(); }

    // unique path
    let shot_dir = env::var("SHOT_DIR").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/Pictures/Screenshots", home)
    });
    let _ = std::fs::create_dir_all(&shot_dir);
    let stamp = chrono_like_stamp();
    let slug = window_slug();
    let base = if slug.is_empty() {
        format!("screenshot-{}", stamp)
    } else {
        format!("screenshot-{}-{}", stamp, slug)
    };
    let mut candidate = format!("{}/{}.png", shot_dir, base);
    let mut suffix = 1;
    while std::path::Path::new(&candidate).exists() {
        candidate = format!("{}/{}-{}.png", shot_dir, base, suffix);
        suffix += 1;
    }

    // grim to captured file (for save/preview) or scratch (for copy)
    let tmp_scratch = format!("/tmp/anixie-shot-{}.png", std::process::id());
    let (code, _, err) = run(&grim(), &["-c", "-g", &geometry, &tmp_scratch]);
    if code != 0 {
        release_freeze();
        let _ = std::fs::remove_file(&tmp_scratch);
        return format!("ERR grim: {}", err.trim());
    }

    let last_capture_file = runtime_dir().join("last-capture");

    match dest {
        "copy" => {
            // Try to put it on the clipboard; on failure, save instead.
            let code = std::process::Command::new("sh")
                .args(["-c", &format!("{} --type image/png < {}", wl_copy(), tmp_scratch)])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            if code == 0 {
                let _ = std::fs::write(&last_capture_file, format!("{}\n", tmp_scratch));
                release_freeze();
                let _ = std::fs::remove_file(&tmp_scratch);
                "OK copied".to_string()
            } else {
                let _ = move_file(&tmp_scratch, &candidate);
                let _ = std::fs::write(&last_capture_file, format!("{}\n", candidate));
                release_freeze();
                let _ = run(&notify(), &["-a", "capture", "-u", "normal", "-t", "8000", "Screenshot saved, not copied", "The clipboard could not be reached."]);
                format!("ERR clipboard unavailable; saved to {}", candidate)
            }
        }
        "save" => {
            let _ = move_file(&tmp_scratch, &candidate);
            let _ = std::fs::write(&last_capture_file, format!("{}\n", candidate));
            release_freeze();
            let _ = run(&notify(), &["-a", "capture", "-u", "low", "-t", "2000", "Screenshot saved", &candidate]);
            candidate
        }
        "preview" => {
            let _ = move_file(&tmp_scratch, &candidate);
            let _ = std::fs::write(&last_capture_file, format!("{}\n", candidate));
            release_freeze();
            let mut copied = true;
            let code = std::process::Command::new("sh")
                .args(["-c", &format!("{} --type image/png < {}", wl_copy(), candidate)])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            if code != 0 { copied = false; }
            let ipc_action = if copied { "preview" } else { "previewCopyFailed" };
            let (ipc_code, _, _) = run("quickshell", &["ipc", "call", "capture", ipc_action, &candidate]);
            if ipc_code != 0 {
                let _ = run(&notify(), &["-a", "capture", "-u", "low", "-t", "4000", "Screenshot saved", &candidate]);
            }
            if copied { format!("Saved {}", candidate) } else { format!("Saved {} (not copied)", candidate) }
        }
        _ => {
            release_freeze();
            let _ = std::fs::remove_file(&tmp_scratch);
            format!("ERR unknown destination: {}", dest)
        }
    }
}

fn release_freeze() {
    let freeze_pid_file = runtime_dir().join("anixie-pick-freeze.pid");
    if let Ok(pid) = std::fs::read_to_string(&freeze_pid_file) {
        let pid = pid.trim();
        if !pid.is_empty() {
            let _ = run("kill", &[pid]);
        }
    }
    let _ = std::fs::remove_file(&freeze_pid_file);
}

fn window_slug() -> String {
    let (code, out, _) = run(&hyprctl(), &["activewindow", "-j"]);
    if code != 0 { return String::new(); }
    let filter = r#".class // empty"#;
    let script = format!("printf '%s' {} | jq -r {}", shell_quote(&out), shell_quote(filter));
    let (code, g, _) = run("sh", &["-c", &script]);
    if code != 0 { return String::new(); }
    let class = g.trim();
    if class.is_empty() { return String::new(); }
    let lowered = class.to_lowercase();
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in lowered.chars() {
        if c.is_alphanumeric() {
            slug.push(c);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    slug.chars().take(40).collect()
}

fn chrono_like_stamp() -> String {
    // use date command
    let (code, out, _) = run("date", &["+%Y-%m-%d_%H-%M-%S"]);
    if code == 0 { out.trim().to_string() } else { "0".to_string() }
}

