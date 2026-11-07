//! Webcam overlay: mpv window over the record region.
//!
//! Split from a single 299-line webcam_overlay.rs:
//! - `overlay` — device listing, start/stop
//! - `resize` — window resize/move, ladder targets
//!
//! `handle()` below is just dispatch.

mod overlay;
mod resize;

use crate::proc::{resolve_tool, run};
use std::env;
use std::path::PathBuf;

fn runtime_dir() -> PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        
        env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        })
    });
    PathBuf::from(rt).join("anixie")
}

fn gsr() -> String { env::var("GSR").unwrap_or_else(|_| "gpu-screen-recorder".to_string()) }
fn hyprctl() -> String { "hyprctl".to_string() }
fn mpv() -> String { env::var("MPV").unwrap_or_else(|_| "mpv".to_string()) }

pub(crate) const MARGIN: i64 = 40;

pub(crate) use overlay::{list_devices, start_overlay, stop_overlay};
#[allow(unused_imports)]
pub(crate) use resize::{resize, resize_target};

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
            if !matches!(action.as_str(), "small" | "medium" | "large" | "smaller" | "larger" | "reset") {
                return format!("ERR unknown resize action: {} (try: small, medium, large, smaller, larger, reset)", action);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_target_ladder() {
        let small = (100, 90);
        let medium = (200, 180);
        let large = (300, 270);
        assert_eq!(resize_target("small", 0, 0, small, medium, large), Some(small));
        assert_eq!(resize_target("reset", 999, 999, small, medium, large), Some(medium));
        assert_eq!(resize_target("bogus", 0, 0, small, medium, large), None);
        // smaller/larger step by width, skipping the current size
        assert_eq!(resize_target("smaller", 250, 225, small, medium, large), Some(medium));
        assert_eq!(resize_target("larger", 150, 135, small, medium, large), Some(medium));
        // already smallest/largest: unchanged
        assert_eq!(resize_target("smaller", 100, 90, small, medium, large), Some((100, 90)));
    }

    #[test]
    fn resize_unknown_action_is_err() {
        let out = handle(&["resize".to_string(), "bogus".to_string()]);
        assert!(out.starts_with("ERR unknown resize action:"), "got: {out}");
    }

    #[test]
    fn start_unknown_size_is_err() {
        let out = handle(&["start".to_string(), "huge".to_string()]);
        assert!(out.starts_with("ERR unknown size:"), "got: {out}");
    }
}
