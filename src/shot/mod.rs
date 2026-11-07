//! Screenshots.
//!
//! Split from a single 423-line shot.rs for readability:
//! - `geometry` — hyprctl JSON geometry, window slug, timestamp
//! - `capture` — grim capture to save/copy/preview/annotate, freeze
//! - `db` — asset index (wabi `media_db` port): add/list/clear/thumbs
//!
//! `handle()` below is just argument parsing/dispatch.

pub mod geometry;
pub mod capture;
pub mod scroll;
pub mod db;

use crate::proc::run;
use std::env;
use std::process::Command;

pub(crate) use crate::proc::wl_copy;
pub(crate) use crate::proc::notify_send as notify;

pub(crate) fn shot_pick() -> String { env::var("SHOT_PICK").unwrap_or_else(|_| "shot-pick".to_string()) }
pub(crate) fn grim() -> String { env::var("GRIM").unwrap_or_else(|_| "grim".to_string()) }
pub(crate) fn hyprctl() -> String { "hyprctl".to_string() }
pub(crate) fn runtime_dir() -> std::path::PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        
        env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        })
    });
    std::path::PathBuf::from(rt).join("anixie")
}

pub fn handle(args: &[String]) -> String {
    // `shot db ...` indexes assets/colors/OCR (wabi media_db).
    if args.first().map(|s| s.as_str()) == Some("db") {
        let rest: Vec<String> = args[1..].iter().map(|s| s.to_string()).collect();
        return db::handle(&rest);
    }
    if args.is_empty() {
        return capture::handle_mode_dest("smart", "preview", &[], None, &[]);
    }
    let mut mode = "smart".to_string();
    let mut dest = "preview".to_string();
    let mut post: Vec<String> = Vec::new();
    let mut geometry: Option<String> = None;
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
            _ if geometry.is_none() && parse_geometry(a).is_some() => { geometry = Some(a.to_string()); }
            _ => { rest.push(a.to_string()); }
        }
        i += 1;
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

    // `-- CMD` runs CMD on the kept capture, like legacy exec.
    // It only makes sense with `save` (copy keeps nothing, annotate
    // keeps a different file).
    if !post.is_empty() && dest != "save" {
        return format!("ERR shot: -- CMD cannot be combined with \"{}\"", dest);
    }

    // An explicit geometry (from the AreaPicker, which already did
    // the picking) skips shot-pick: `shot region "X,Y WxH" <dest>`.
    if geometry.is_some() && mode != "region" {
        return "ERR geometry is only valid for region mode".to_string();
    }

    capture::handle_mode_dest(&mode, &dest, &rest, geometry.as_deref(), &post)
}

/// Validate a grim-style geometry "X,Y WxH" (physical pixels, X/Y may
/// be negative on multi-monitor layouts). Pure, so it is testable.
pub(crate) fn parse_geometry(s: &str) -> Option<String> {
    let (pos, size) = s.split_once(' ')?;
    let (x, y) = pos.split_once(',')?;
    x.parse::<i64>().ok()?;
    y.parse::<i64>().ok()?;
    let (w, h) = size.split_once('x')?;
    let w: i64 = w.parse().ok()?;
    let h: i64 = h.parse().ok()?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(s.to_string())
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

    #[test]
    fn parse_geometry_validates() {
        assert_eq!(parse_geometry("100,100 700x400"), Some("100,100 700x400".to_string()));
        assert_eq!(parse_geometry("-100,50 700x400"), Some("-100,50 700x400".to_string()));
        assert_eq!(parse_geometry("100,100 0x400"), None);
        assert_eq!(parse_geometry("100,100"), None);
        assert_eq!(parse_geometry("__bogus__"), None);
    }
}
