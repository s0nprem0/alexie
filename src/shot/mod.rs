//! Screenshots.
//!
//! Split from a single 423-line shot.rs for readability:
//! - `geometry` — hyprctl JSON geometry, window slug, timestamp
//! - `capture` — grim capture to save/copy/preview/annotate, freeze
//!
//! `handle()` below is just argument parsing/dispatch.

pub mod geometry;
pub mod capture;

use crate::proc::run;
use std::env;
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

pub(crate) fn shot_pick() -> String { env::var("SHOT_PICK").unwrap_or_else(|_| "shot-pick".to_string()) }
pub(crate) fn grim() -> String { env::var("GRIM").unwrap_or_else(|_| "grim".to_string()) }
pub(crate) fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }
pub(crate) fn notify() -> String { env::var("NOTIFY_SEND").unwrap_or_else(|_| "notify-send".to_string()) }
pub(crate) fn hyprctl() -> String { "hyprctl".to_string() }
pub(crate) fn runtime_dir() -> std::path::PathBuf {
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
        return capture::handle_mode_dest("smart", "preview", &[]);
    }
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
            "--" => { after_dash = true; }
            _ => { rest.push(a.to_string()); }
        }
        i += 1;
    }

    if mode == "scroll" {
        return delegate(args);
    }

    if let Some(unknown) = rest.first() {
        return format!("ERR unknown argument: {}", unknown);
    }

    if mode == "cancel" {
        let runtime = runtime_dir();
        let _ = std::fs::create_dir_all(&runtime);
        let _ = std::fs::write(runtime.join("shot-scroll-cancel"), "");
        let _ = run("quickshell", &["ipc", "call", "capture", "hide"]);
        return "OK".to_string();
    }

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

    if mode == "copy-path" {
        let last_file = runtime_dir().join("last-capture");
        let last = std::fs::read_to_string(&last_file).unwrap_or_default().trim().to_string();
        if last.is_empty() || !std::path::Path::new(&last).exists() {
            return "ERR no previous capture".to_string();
        }
        let _ = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/plain", &last, &[]);
        let _ = run("notify-send", &["-a", "capture", "-u", "low", "-t", "2000", "Copied path", &last]);
        return last;
    }

    if mode == "copy-uri" {
        let last_file = runtime_dir().join("last-capture");
        let last = std::fs::read_to_string(&last_file).unwrap_or_default().trim().to_string();
        if last.is_empty() || !std::path::Path::new(&last).exists() {
            return "ERR no previous capture".to_string();
        }
        let uri = format!("file://{}", last);
        let _ = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/uri-list", &uri, &[]);
        let _ = run("notify-send", &["-a", "capture", "-u", "low", "-t", "2000", "Copied file URI", &uri]);
        return uri;
    }

    if !post.is_empty() {
        return delegate(args);
    }

    capture::handle_mode_dest(&mode, &dest, &rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_argument_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown argument:"), "got: {out}");
        let out = handle(&["region".to_string(), "__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown argument:"), "got: {out}");
    }
}
