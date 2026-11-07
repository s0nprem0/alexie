//! Grim capture: handle_mode_dest, freeze release, file moves.

use super::geometry::{
    chrono_like_stamp, focused_monitor_geometry_from_json, window_geometry_from_json, window_slug,
};
use super::{grim, hyprctl, notify, runtime_dir, shot_pick, wl_copy};
use crate::proc::{run, run_timeout};
use std::env;
use std::fs;
use std::process::Command;

pub(crate) fn move_file(src: &str, dst: &str) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dst)?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

/// Geometry "X,Y WxH" for the focused window, from hyprctl JSON.
/// Pure, so it can be tested without hyprctl/jq.
pub(crate) fn handle_mode_dest(
    mode: &str,
    dest: &str,
    _rest: &[String],
    geometry_arg: Option<&str>,
    post: &[String],
) -> String {
    // get geometry
    let geometry = if let Some(g) = geometry_arg {
        g.to_string()
    } else if mode == "window" {
        let (code, out, _) = run(&hyprctl(), &["activewindow", "-j"]);
        if code != 0 {
            return "ERR no focused window".to_string();
        }
        match window_geometry_from_json(&out) {
            Some(g) => g,
            None => {
                let _ = run(
                    &notify(),
                    &[
                        "-a",
                        "capture",
                        "-u",
                        "normal",
                        "-t",
                        "6000",
                        "Screenshot failed",
                        "No focused window.",
                    ],
                );
                return "ERR no focused window to capture".to_string();
            }
        }
    } else if mode == "fullscreen" {
        let (code, out, _) = run(&hyprctl(), &["monitors", "-j"]);
        if code != 0 {
            return "ERR no monitors".to_string();
        }
        match focused_monitor_geometry_from_json(&out) {
            Some(g) => g,
            None => {
                let _ = run(
                    &notify(),
                    &[
                        "-a",
                        "capture",
                        "-u",
                        "normal",
                        "-t",
                        "6000",
                        "Screenshot failed",
                        "No focused monitor.",
                    ],
                );
                return "ERR no focused monitor".to_string();
            }
        }
    } else {
        // region / windows: use shot-pick
        let picker_mode = if mode == "windows" {
            "windows"
        } else if mode == "smart" {
            "smart"
        } else {
            "region"
        };
        let sp = shot_pick();
        let (code, g, err) = run(&sp, &[picker_mode, "--keep-freeze"]);
        if code != 0 {
            // Picker failed/cancelled: the freeze is still up.
            // Legacy shot.sh traps this before the picker runs.
            release_freeze();
            let _ = run(
                &notify(),
                &[
                    "-a",
                    "capture",
                    "-u",
                    "normal",
                    "-t",
                    "6000",
                    "Screenshot failed",
                    &format!("Could not pick region: {}", err.trim()),
                ],
            );
            return format!("ERR pick: {}", err.trim());
        }
        let g = g.trim().to_string();
        if g.is_empty() {
            release_freeze();
            let _ = run(
                &notify(),
                &[
                    "-a",
                    "capture",
                    "-u",
                    "normal",
                    "-t",
                    "6000",
                    "Screenshot failed",
                    "No region selected.",
                ],
            );
            return "ERR no region".to_string();
        }
        g
    };

    if geometry.is_empty() {
        return "ERR no geometry".to_string();
    }

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

    // grim to captured file (for save/preview) or scratch (for copy);
    // scrolling capture pages through the window and stitches instead.
    let tmp_scratch = crate::proc::tmp_file("shot", "png")
        .to_string_lossy()
        .to_string();
    if mode == "scroll" {
        if let Err(e) = super::scroll::scroll_capture(&geometry, &tmp_scratch) {
            release_freeze();
            let _ = std::fs::remove_file(&tmp_scratch);
            let _ = run(
                &notify(),
                &[
                    "-a",
                    "capture",
                    "-u",
                    "normal",
                    "-t",
                    "6000",
                    "Screenshot failed",
                    &e,
                ],
            );
            return format!("ERR {}", e);
        }
    } else {
        let (code, _, err) = run(&grim(), &["-c", "-g", &geometry, &tmp_scratch]);
        if code != 0 {
            release_freeze();
            let _ = std::fs::remove_file(&tmp_scratch);
            return format!("ERR grim: {}", err.trim());
        }
    }

    let last_capture_file = runtime_dir().join("last-capture");

    match dest {
        "redact" | "text" | "annotate" => {
            let tool = if dest == "redact" {
                "blur"
            } else if dest == "text" {
                "text"
            } else {
                "crop"
            };
            let suffix = if dest == "redact" {
                "redacted"
            } else if dest == "text" {
                "text"
            } else {
                "annotated"
            };
            let stem = format!("{}-{}", base, suffix);
            // unique path
            let _ = fs::create_dir_all(&shot_dir);
            let mut output = format!("{}/{}.png", shot_dir, stem);
            let mut n = 1;
            while std::path::Path::new(&output).exists() {
                output = format!("{}/{}-{}.png", shot_dir, stem, n);
                n += 1;
            }
            // Unfreeze before opening the GUI annotator: the freeze frame
            // is a fullscreen layer that swallows clicks, so while satty
            // holds input it must not be on top of it.
            release_freeze();
            let satty_bin = env::var("SATTY").unwrap_or_else(|_| "satty".to_string());
            let status = Command::new(&satty_bin)
                .args([
                    "--filename",
                    &tmp_scratch,
                    "--output-filename",
                    &output,
                    "--initial-tool",
                    tool,
                ])
                .status();
            let code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            if code != 0 {
                release_freeze();
                let _ = fs::remove_file(&tmp_scratch);
                return "OK".to_string();
            }
            let _ = fs::write(&last_capture_file, format!("{}\n", output));
            super::db::index_screenshot(&output);
            release_freeze();
            let _ = fs::remove_file(&tmp_scratch);
            if crate::proc::copy_file_to_clipboard(
                &wl_copy(),
                "image/png",
                std::path::Path::new(&output),
            ) != 0
            {
                let _ = run(
                    &notify(),
                    &[
                        "-a",
                        "capture",
                        "-u",
                        "normal",
                        "-t",
                        "8000",
                        "Screenshot saved, not copied",
                        "The clipboard could not be reached.",
                    ],
                );
                return format!("ERR clipboard unavailable; saved to {}", output);
            }
            // Bounded: a hung quickshell handler must not leave the
            // daemon thread (or the frozen overlay) waiting forever.
            let _ = run_timeout(
                "quickshell",
                &["ipc", "call", "capture", "preview", &output],
                std::time::Duration::from_secs(3),
            );
            format!("Saved {}", output)
        }
        "copy" => {
            // Try to put it on the clipboard; on failure, save instead.
            let code = crate::proc::copy_file_to_clipboard(
                &wl_copy(),
                "image/png",
                std::path::Path::new(&tmp_scratch),
            );
            if code == 0 {
                let _ = std::fs::write(&last_capture_file, format!("{}\n", tmp_scratch));
                release_freeze();
                let _ = std::fs::remove_file(&tmp_scratch);
                "OK copied".to_string()
            } else {
                let _ = move_file(&tmp_scratch, &candidate);
                let _ = std::fs::write(&last_capture_file, format!("{}\n", candidate));
                super::db::index_screenshot(&candidate);
                release_freeze();
                let _ = run(
                    &notify(),
                    &[
                        "-a",
                        "capture",
                        "-u",
                        "normal",
                        "-t",
                        "8000",
                        "Screenshot saved, not copied",
                        "The clipboard could not be reached.",
                    ],
                );
                format!("ERR clipboard unavailable; saved to {}", candidate)
            }
        }
        "save" => {
            let _ = move_file(&tmp_scratch, &candidate);
            let _ = std::fs::write(&last_capture_file, format!("{}\n", candidate));
            super::db::index_screenshot(&candidate);
            release_freeze();
            // Legacy exec semantics: the post command replaces the
            // shot process and inherits the terminal, so its output
            // is the command's output, not "Saved ...".
            if !post.is_empty() {
                let mut cmd = Command::new(&post[0]);
                cmd.args(&post[1..]).arg(&candidate);
                return match cmd.status() {
                    Ok(status) if status.success() => "OK".to_string(),
                    Ok(status) => {
                        format!("ERR post command exited {}", status.code().unwrap_or(-1))
                    }
                    Err(e) => format!("ERR spawn post command: {}", e),
                };
            }
            let _ = run(
                &notify(),
                &[
                    "-a",
                    "capture",
                    "-u",
                    "low",
                    "-t",
                    "2000",
                    "Screenshot saved",
                    &candidate,
                ],
            );
            candidate
        }
        "preview" => {
            let _ = move_file(&tmp_scratch, &candidate);
            let _ = std::fs::write(&last_capture_file, format!("{}\n", candidate));
            super::db::index_screenshot(&candidate);
            release_freeze();
            let mut copied = true;
            let code = crate::proc::copy_file_to_clipboard(
                &wl_copy(),
                "image/png",
                std::path::Path::new(&candidate),
            );
            if code != 0 {
                copied = false;
            }
            let ipc_action = if copied {
                "preview"
            } else {
                "previewCopyFailed"
            };
            let (ipc_code, _, _) = run_timeout(
                "quickshell",
                &["ipc", "call", "capture", ipc_action, &candidate],
                std::time::Duration::from_secs(3),
            );
            if ipc_code != 0 {
                // Fallback matching legacy show_capture: an image icon and
                // actions, so the saved file is reachable without the shell.
                let file_dir = std::path::Path::new(&candidate)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let _ = run(
                    &notify(),
                    &[
                        "-a", "capture",
                        "-u", "low",
                        "-t", "6000",
                        "-i", &candidate,
                        "Screenshot saved",
                        &format!("Copied to the clipboard and saved to {}", candidate),
                        "--action", "Open", "xdg-open", &candidate,
                        "--action", "Edit", "shot", "edit-last",
                        "--action", "Show folder", "xdg-open", &file_dir,
                    ],
                );
            }
            if copied {
                format!("Saved {}", candidate)
            } else {
                format!("Saved {} (not copied)", candidate)
            }
        }
        _ => {
            release_freeze();
            let _ = std::fs::remove_file(&tmp_scratch);
            format!("ERR unknown destination: {}", dest)
        }
    }
}

/// Dismiss the fullscreen freeze surface. The legacy freeze was a
/// `slurp`/`swappy` process whose pid lived in `anixie-pick-freeze.pid`;
/// killing it unfreezes the frame. When quickshell itself owns the
/// layer, that kill misses — so also send `capture hide` (best-effort,
/// bounded) so the surface cannot swallow clicks if the pid is stale.
pub(crate) fn release_freeze() {
    let freeze_pid_file = runtime_dir().join("anixie-pick-freeze.pid");
    if let Ok(pid) = std::fs::read_to_string(&freeze_pid_file) {
        let Ok(pid) = pid.trim().parse::<i64>() else {
            let _ = std::fs::remove_file(&freeze_pid_file);
            return;
        };
        if pid > 0 {
            let _ = run("kill", &["--", &pid.to_string()]);
        }
    }
    let _ = std::fs::remove_file(&freeze_pid_file);
    let _ = run_timeout(
        "quickshell",
        &["ipc", "call", "capture", "hide"],
        std::time::Duration::from_secs(2),
    );
}
