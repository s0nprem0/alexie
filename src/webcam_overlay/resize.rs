//! Resize: window geometry from hyprctl clients, anchor from
//! record region, ladder targets.

use super::{runtime_dir, MARGIN};
use super::hyprctl;
use crate::proc::run;
use serde_json::Value;
use std::fs;

pub(crate) fn resize(action: &str) -> String {
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

    let (target_w, target_h) = match resize_target(
        action,
        cur_w,
        cur_h,
        (small_w, small_h),
        (medium_w, medium_h),
        (large_w, large_h),
    ) {
        Some(v) => v,
        None => (cur_w, cur_h),
    };

    let mut target_x = anchor_x + anchor_w - target_w - MARGIN;
    let mut target_y = anchor_y + anchor_h - target_h - MARGIN;
    if target_x < anchor_x + MARGIN { target_x = anchor_x + MARGIN; }
    if target_y < anchor_y + MARGIN { target_y = anchor_y + MARGIN; }

    let window = format!("address:{}", address);
    // Lua dispatcher first, classic dispatcher fallback, like legacy.
    let (code, _, _) = run(&hyprctl(), &["dispatch", &format!("hl.dsp.window.resize({{ window = \"{}\", x = {}, y = {} }})", window, target_w, target_h)]);
    if code != 0 {
        let _ = run(&hyprctl(), &["dispatch", &format!("resizewindowpixel exact {} {},{}", target_w, target_h, window)]);
    }
    let (code, _, _) = run(&hyprctl(), &["dispatch", &format!("hl.dsp.window.move({{ window = \"{}\", x = {}, y = {} }})", window, target_x, target_y)]);
    if code != 0 {
        let _ = run(&hyprctl(), &["dispatch", &format!("movewindowpixel exact {} {},{}", target_x, target_y, window)]);
    }
    "OK".to_string()
}

/// Pure resize-target calculation, mirroring legacy resize():
/// ladder heights are rounded to nearest (not truncated) so the
/// three steps stay even. Returns (w, h) for an action, or None
/// for an unknown action.
pub(crate) fn resize_target(action: &str, cur_w: i64, cur_h: i64, small: (i64, i64), medium: (i64, i64), large: (i64, i64)) -> Option<(i64, i64)> {
    match action {
        "small" => Some(small),
        "medium" | "reset" => Some(medium),
        "large" => Some(large),
        "smaller" => {
            if large.0 < cur_w { Some(large) }
            else if medium.0 < cur_w { Some(medium) }
            else if small.0 < cur_w { Some(small) }
            else { Some((cur_w, cur_h)) }
        }
        "larger" => {
            if small.0 > cur_w { Some(small) }
            else if medium.0 > cur_w { Some(medium) }
            else if large.0 > cur_w { Some(large) }
            else { Some((cur_w, cur_h)) }
        }
        _ => None,
    }
}

