//! Scrolling capture: page through a focused window and stitch frames.
//!
//! Port of the `shot scroll` path in legacy shot.sh. Page Down is used
//! instead of a mouse wheel: it needs no pointer position and every
//! toolkit understands it. The overlap between two frames is found by
//! taking a strip from the previous frame, well above its bottom edge
//! so a sticky footer cannot anchor the match, and searching for it in
//! the new one at quarter size.

use super::{grim, hyprctl, notify, runtime_dir};
use super::capture::release_freeze;
use crate::proc::run;
use std::env;
use std::thread;
use std::time::Duration;

fn magick() -> String { env::var("MAGICK").unwrap_or_else(|_| "magick".to_string()) }
fn wtype() -> String { env::var("WTYPE").unwrap_or_else(|_| "wtype".to_string()) }

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}
fn env_secs(name: &str, default: f64) -> Duration {
    let secs: f64 = env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default);
    Duration::from_secs_f64(secs.max(0.0))
}

/// Parse `compare -metric RMSE -subimage-search` stderr, whose shape is
/// "1234.5 (0.0123) @ 0,120": the bracketed score and the y offset of
/// the match. Pure, so it can be tested without ImageMagick.
pub(crate) fn parse_compare_metric(stderr: &str) -> Option<(f64, i64)> {
    let line = stderr.lines().next()?;
    let open = line.find('(')?;
    let close = line[open..].find(')')? + open;
    let threshold: f64 = line[open + 1..close].trim().parse().ok()?;
    let at = line.rfind("@ ")?;
    let coords = &line[at + 2..];
    let comma = coords.find(',')?;
    let match_y: i64 = coords[comma + 1..].split_whitespace().next()?.parse().ok()?;
    Some((threshold, match_y))
}

/// Pixels the two frames share, from the strip's position in each:
/// the strip started at `strip_top` in the previous frame and appears
/// at `match_y` in the current one, so the scroll distance is their
/// difference (at full resolution) and everything after it is new.
/// Pure, so it can be tested without ImageMagick.
pub(crate) fn overlap_pixels(previous_height: i64, strip_top: i64, match_y: i64, scale: i64) -> Option<i64> {
    let scroll_distance = (strip_top - match_y) * scale;
    let overlap = previous_height - scroll_distance;
    if overlap > 0 && overlap < previous_height {
        Some(overlap)
    } else {
        None
    }
}

fn image_size(path: &std::path::Path) -> Option<(i64, i64)> {
    let (code, out, _) = run(&magick(), &["identify", "-format", "%w %h", &path.to_string_lossy()]);
    if code != 0 {
        return None;
    }
    let mut parts = out.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// Overlap in full-resolution pixels, or None when there is no usable
/// match (caller appends the frame whole, which at worst repeats the
/// line page-down leaves behind).
fn scroll_overlap(previous: &std::path::Path, current: &std::path::Path, tmp: &std::path::Path) -> Option<i64> {
    let (previous_width, previous_height) = image_size(previous)?;
    let (current_width, _) = image_size(current)?;
    if previous_width != current_width || previous_height <= 0 {
        return None;
    }
    let scale: i64 = if previous_width < 400 { 1 } else { 4 };
    let small_width = previous_width / scale;
    let small_height = previous_height / scale;
    if small_width <= 0 || small_height <= 0 {
        return None;
    }
    let mut strip_height = small_height / 8;
    if strip_height < 8 {
        strip_height = 8;
    }
    if strip_height >= small_height {
        return None;
    }
    let mut strip_top = small_height * 3 / 5;
    if strip_top + strip_height > small_height {
        strip_top = small_height - strip_height;
    }
    let small_previous = tmp.join("previous-small.png");
    let small_current = tmp.join("current-small.png");
    let strip = tmp.join("strip.png");
    let (code, _, _) = run(&magick(), &[&previous.to_string_lossy(), "-resize", &format!("{}x{}!", small_width, small_height), &small_previous.to_string_lossy()]);
    if code != 0 {
        return None;
    }
    let (code, _, _) = run(&magick(), &[&current.to_string_lossy(), "-resize", &format!("{}x{}!", small_width, small_height), &small_current.to_string_lossy()]);
    if code != 0 {
        return None;
    }
    let (code, _, _) = run(&magick(), &[&small_previous.to_string_lossy(), "-crop", &format!("{}x{}+0+{}", small_width, strip_height, strip_top), "+repage", &strip.to_string_lossy()]);
    if code != 0 {
        return None;
    }
    // `compare` exits 1 when the images differ, which is the normal
    // case. Its metric goes to stderr: "1234.5 (0.0123) @ 0,120".
    let (_, _, err) = run(&magick(), &["compare", "-metric", "RMSE", "-subimage-search", &small_current.to_string_lossy(), &strip.to_string_lossy(), "null:"]);
    let (threshold, match_y) = parse_compare_metric(&err)?;
    // 0.08 is generous on purpose: JPEG-ish UI noise and subpixel
    // rendering move the score a little even when the scroll position
    // is exactly right.
    if threshold > 0.08 {
        return None;
    }
    overlap_pixels(previous_height, strip_top, match_y, scale)
}

/// Append the new part of `frame` to `stitched`, skipping the overlap.
/// A missing overlap means "append it whole".
fn scroll_append(stitched: &std::path::Path, frame: &std::path::Path, overlap: i64, tmp: &std::path::Path) -> bool {
    let Some((frame_width, frame_height)) = image_size(frame) else {
        return false;
    };
    
    let part = if overlap > 0 && overlap < frame_height {
        let part_path = tmp.join("part.png");
        let (code, _, _) = run(&magick(), &[&frame.to_string_lossy(), "-crop", &format!("{}x{}+0+{}", frame_width, frame_height - overlap, overlap), "+repage", &part_path.to_string_lossy()]);
        if code != 0 {
            return false;
        }
        part_path
    } else {
        frame.to_path_buf()
    };
    let next = tmp.join("next.png");
    let (code, _, _) = run(&magick(), &[&stitched.to_string_lossy(), &part.to_string_lossy(), "-append", &next.to_string_lossy()]);
    if code != 0 {
        return false;
    }
    std::fs::rename(&next, stitched).is_ok()
}

fn focused_address() -> String {
    let (code, out, _) = run(&hyprctl(), &["activewindow", "-j"]);
    if code != 0 {
        return String::new();
    }
    serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .and_then(|v| v.get("address").and_then(|a| a.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Page through the focused window, stitching frames into `output`.
/// Err when nothing was captured at all.
pub(crate) fn scroll_capture(geometry: &str, output: &str) -> Result<(), String> {
    release_freeze();

    let scroll_frames = env_usize("SCROLL_FRAMES", 16);
    let scroll_presses = env_usize("SCROLL_PRESSES", 1);
    let scroll_key_delay = env_secs("SCROLL_KEY_DELAY", 0.08);
    let scroll_settle = env_secs("SCROLL_SETTLE", 0.45);
    let scroll_max_height: i64 = env_usize("SCROLL_MAX_HEIGHT", 24000) as i64;

    let cancel_file = runtime_dir().join("shot-scroll-cancel");
    let _ = std::fs::remove_file(&cancel_file);

    let tmp = std::env::temp_dir().join(format!(
        "shot-scroll-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if std::fs::create_dir_all(&tmp).is_err() {
        return Err("Scrolling capture could not create a temp directory.".to_string());
    }
    let tmp = tmp.as_path();

    // Page Down goes to whatever has focus, not to the window that was
    // picked. The address is taken once, before the first key, and
    // checked before every frame; a window that was closed or replaced
    // stops the loop with the frames captured so far instead of
    // scrolling something else.
    let scroll_focus = focused_address();

    let _ = run(&notify(), &["-a", "capture", "-u", "low", "-t", "4000", "Scrolling capture", "Leave the window focused. Super+Esc stops it and keeps what is captured."]);

    let mut stop = "moving";
    let mut i = 0usize;
    let mut previous: Option<std::path::PathBuf> = None;
    while i < scroll_frames {
        if cancel_file.exists() {
            stop = "cancelled";
            break;
        }
        if !scroll_focus.is_empty() && focused_address() != scroll_focus {
            stop = "focus";
            break;
        }
        let current = tmp.join(format!("frame-{}.png", i));
        let (code, _, _) = run(&grim(), &["-g", geometry, &current.to_string_lossy()]);
        if code != 0 || std::fs::metadata(&current).map(|m| m.len()).unwrap_or(0) == 0 {
            break;
        }
        let stitched = tmp.join("stitched.png");
        if let Some(prev) = previous.as_ref() {
            // `compare -metric AE` prints "0 (0)" when nothing moved.
            let (_, _, err) = run(&magick(), &["compare", "-metric", "AE", &prev.to_string_lossy(), &current.to_string_lossy(), "null:"]);
            let unchanged = err.trim();
            if unchanged == "0 (0)" || unchanged == "0" {
                stop = "settled";
                break;
            }
            let overlap = scroll_overlap(prev, &current, tmp).unwrap_or(0);
            if !scroll_append(&stitched, &current, overlap, tmp) {
                break;
            }
            let height = image_size(&stitched).map(|(_, h)| h).unwrap_or(0);
            if height >= scroll_max_height {
                stop = "height";
                break;
            }
        } else {
            let _ = std::fs::copy(&current, &stitched);
        }
        previous = Some(current);
        i += 1;
        if i >= scroll_frames {
            stop = "frames";
            break;
        }
        for _ in 0..scroll_presses {
            let (code, _, _) = run(&wtype(), &["-k", "Next"]);
            if code != 0 {
                break;
            }
            thread::sleep(scroll_key_delay);
        }
        thread::sleep(scroll_settle);
    }

    let _ = std::fs::remove_file(&cancel_file);

    let stitched = tmp.join("stitched.png");
    if std::fs::metadata(&stitched).map(|m| m.len()).unwrap_or(0) == 0 {
        let _ = std::fs::remove_dir_all(tmp);
        return Err("Scrolling capture produced nothing. The page may not have moved, or the window lost focus.".to_string());
    }
    if std::fs::copy(&stitched, output).is_err() {
        let _ = std::fs::remove_dir_all(tmp);
        return Err("Scrolling capture could not write the stitched image.".to_string());
    }
    let _ = std::fs::remove_dir_all(tmp);

    let (note, urgency) = match stop {
        "cancelled" => ("Cancelled. The part captured so far was kept.", "normal"),
        "focus" => ("Focus moved to another window. The part captured so far was kept.", "normal"),
        "height" => ("Stopped at the height limit. The page continues below what was captured.", "normal"),
        "frames" => ("Stopped after 16 frames. The page may continue further.", "normal"),
        _ => ("Done. The page stopped moving.", "low"),
    };
    // Name the frame limit in the note when that is what stopped us,
    // with the configured value rather than a baked-in one.
    let note = if stop == "frames" {
        format!("Stopped after {} frames. The page may continue further.", scroll_frames)
    } else {
        note.to_string()
    };
    let _ = run(&notify(), &["-a", "capture", "-u", urgency, "-t", "8000", "Scrolling capture", &note]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_metric_parses_score_and_offset() {
        assert_eq!(parse_compare_metric("1234.5 (0.0123) @ 0,120\n"), Some((0.0123, 120)));
        assert_eq!(parse_compare_metric("4.2 (0.5) @ 10,20"), Some((0.5, 20)));
        assert_eq!(parse_compare_metric("no metric here"), None);
        assert_eq!(parse_compare_metric(""), None);
    }

    #[test]
    fn overlap_pixels_math() {
        // strip at 300 (quarter-size), scale 4, found at y=100 in the
        // next frame: scrolled 800px, 600px still shared.
        assert_eq!(overlap_pixels(1400, 300, 100, 4), Some(600));
        // A strip found at the same offset means nothing scrolled:
        // treated as no usable match (the AE check catches settled
        // frames first), like the legacy strict comparison.
        assert_eq!(overlap_pixels(1400, 300, 300, 4), None);
        assert_eq!(overlap_pixels(1400, 300, 400, 4), None);
    }
}
