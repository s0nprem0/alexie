use std::env;
use std::fs;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            (code, out, err)
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn delegate(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, out, err) = run("shot", &a);
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
    std::path::PathBuf::from(rt)
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
            "smart" | "region" | "windows" | "fullscreen" | "window" | "scroll" => { mode = a.to_string(); }
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

    // delegate complex modes/dests
    if mode == "smart" || mode == "scroll" || dest == "redact" || dest == "text" || dest == "annotate" {
        return delegate(args);
    }
    if !post.is_empty() {
        return delegate(args);
    }

    handle_mode_dest(&mode, &dest, &rest)
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
        if code != 0 || g.trim().is_empty() { return "ERR no focused window to capture".to_string(); }
        g.trim().to_string()
    } else if mode == "fullscreen" {
        // focused monitor geometry: 0,0 WxH
        let (code, out, _) = run(&hyprctl(), &["monitors", "-j"]);
        if code != 0 { return "ERR no monitors".to_string(); }
        let filter = r#"[.[] | select(.focused == true)][0] | "\(.x),\(.y) \(.width)x\(.height)""#;
        let script = format!("printf '%s' {} | jq -r '{}'", shell_quote(&out), filter);
        let (code, g, _) = run("sh", &["-c", &script]);
        if code != 0 || g.trim().is_empty() { return "ERR no focused monitor".to_string(); }
        g.trim().to_string()
    } else {
        // region / windows: use shot-pick
        let picker_mode = if mode == "windows" { "windows" } else { "region" };
        let sp = shot_pick();
        let (code, g, err) = run(&sp, &[picker_mode, "--keep-freeze"]);
        if code != 0 { return format!("ERR pick: {}", err.trim()); }
        let g = g.trim().to_string();
        if g.is_empty() { return "ERR no region".to_string(); }
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
    let base = format!("Screenshot_{}", stamp);
    let mut candidate = format!("{}/{}.png", shot_dir, base);
    let mut suffix = 1;
    while std::path::Path::new(&candidate).exists() {
        candidate = format!("{}/{}-{}.png", shot_dir, base, suffix);
        suffix += 1;
    }

    // grim to captured file (for save/preview) or scratch (for copy)
    let tmp_scratch = format!("/tmp/anixie-shot-{}.png", std::process::id());
    let (code, _, err) = run(&grim(), &["-g", &geometry, &tmp_scratch]);
    if code != 0 {
        release_freeze();
        let _ = std::fs::remove_file(&tmp_scratch);
        return format!("ERR grim: {}", err.trim());
    }

    let last_capture_file = runtime_dir().join("last-capture");

    match dest {
        "copy" => {
            let _ = run("sh", &["-c", &format!("{} --type image/png < {}", wl_copy(), tmp_scratch)]);
            let _ = std::fs::write(&last_capture_file, &tmp_scratch);
            release_freeze();
            // cleanup scratch? keep for last-capture? bash keeps nothing for copy. Remove scratch.
            let _ = std::fs::remove_file(&tmp_scratch);
            let _ = std::fs::remove_file(&last_capture_file);
            "OK copied".to_string()
        }
        "save" => {
            let _ = std::fs::rename(&tmp_scratch, &candidate);
            let _ = std::fs::write(&last_capture_file, &candidate);
            release_freeze();
            let _ = run(&notify(), &["-a", "capture", "-u", "low", "-t", "2000", "Screenshot saved", &candidate]);
            candidate
        }
        "preview" => {
            let _ = std::fs::rename(&tmp_scratch, &candidate);
            let _ = std::fs::write(&last_capture_file, &candidate);
            release_freeze();
            // copy to clipboard
            let _ = run("sh", &["-c", &format!("{} --type image/png < {}", wl_copy(), candidate)]);
            // reveal in capture preview
            let _ = run("quickshell", &["ipc", "call", "capture", "preview", &candidate]);
            candidate
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

fn chrono_like_stamp() -> String {
    // use date command
    let (code, out, _) = run("date", &["+%Y-%m-%d_%H-%M-%S"]);
    if code == 0 { out.trim().to_string() } else { "0".to_string() }
}

fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
}
