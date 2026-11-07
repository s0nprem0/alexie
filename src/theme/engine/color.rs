use std::path::Path;

// ---------- palette validation & colour maths (port of color.sh) ----------

pub(crate) fn color_ok(s: &str) -> bool {
    s.len() == 7
        && s.starts_with('#')
        && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn hex_byte(h: &str, i: usize) -> u8 {
    u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0)
}

fn lin(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub(crate) fn luminance(hex: &str) -> Option<f64> {
    if !color_ok(hex) {
        return None;
    }
    let h = &hex[1..];
    let r = lin(hex_byte(h, 0));
    let g = lin(hex_byte(h, 2));
    let b = lin(hex_byte(h, 4));
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

pub(crate) fn contrast(a: &str, b: &str) -> Option<f64> {
    let mut la = luminance(a)?;
    let mut lb = luminance(b)?;
    if la < lb {
        std::mem::swap(&mut la, &mut lb);
    }
    Some((la + 0.05) / (lb + 0.05))
}

fn mix(a: &str, b: &str, t: f64) -> String {
    let t = t.clamp(0.0, 1.0);
    let ah = &a[1..];
    let bh = &b[1..];
    let mut out = String::from("#");
    for i in (0..6).step_by(2) {
        let av = hex_byte(ah, i) as f64;
        let bv = hex_byte(bh, i) as f64;
        let ch = av + (bv - av) * t;
        out.push_str(&format!("{:02x}", (ch.round() as i64).clamp(0, 255)));
    }
    out
}

/// Port of color_adjust_contrast(): mix fg toward black or white
/// (whichever contrasts more with bg) until the target ratio is
/// reached, via binary search. Returns None if unreachable or
/// inputs are not colours; returns the original if it already passes.
pub(crate) fn adjust_contrast(fg: &str, bg: &str, want: f64) -> Option<String> {
    if !color_ok(fg) || !color_ok(bg) || want <= 0.0 {
        return None;
    }
    if contrast(fg, bg)? >= want {
        return Some(fg.to_string());
    }
    let c_black = contrast("#000000", bg)?;
    let c_white = contrast("#ffffff", bg)?;
    let endpoint = if c_black >= c_white { "#000000" } else { "#ffffff" };
    if contrast(endpoint, bg)? < want {
        return None;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        let cand = mix(fg, endpoint, mid);
        if contrast(&cand, bg)? >= want {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(mix(fg, endpoint, hi))
}

const PALETTE_KEYS: &[&str] = &[
    "background", "surface", "surfaceContainer", "surfaceVariant",
    "foreground", "fgLight", "bgDark", "bgLight", "muted", "outline",
    "primary", "accent", "accentAlt", "secondary", "tertiary",
    "success", "yellow", "danger", "error",
];

pub(crate) fn palette_ok_value(v: &serde_json::Value) -> bool {
    if !v.is_object() {
        return false;
    }
    PALETTE_KEYS.iter().all(|k| {
        v.get(*k)
            .and_then(|x| x.as_str())
            .map(color_ok)
            .unwrap_or(false)
    })
}

pub(crate) fn palette_ok_file(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| palette_ok_value(&v))
        .unwrap_or(false)
}

/// Port of color_palette_fix(): adjust foreground/fgLight/muted/
/// outline against background/surface to 4.5 in place.
pub(crate) fn palette_fix_value(v: &mut serde_json::Value) -> Result<(), String> {
    if !v.is_object() {
        return Err("palette is not an object".to_string());
    }
    for key in ["foreground", "fgLight", "muted", "outline"] {
        let Some(original) = v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string()) else {
            continue;
        };
        if !color_ok(&original) {
            return Err(format!("invalid colour for {key}"));
        }
        let mut current = original.clone();
        for bg_key in ["background", "surface"] {
            let Some(bg) = v.get(bg_key).and_then(|x| x.as_str()).map(|s| s.to_string()) else {
                continue;
            };
            if !color_ok(&bg) {
                return Err(format!("invalid colour for {bg_key}"));
            }
            let fixed = adjust_contrast(&current, &bg, 4.5)
                .ok_or_else(|| format!("could not adjust {key} against {bg_key}"))?;
            current = fixed;
        }
        if current != original
            && let Some(obj) = v.as_object_mut() {
                obj.insert(key.to_string(), serde_json::json!(current));
            }
    }
    Ok(())
}

pub(crate) fn color_accent_name(hex: &str) -> Option<String> {
    if !color_ok(hex) {
        return None;
    }
    let h = &hex[1..];
    let r = hex_byte(h, 0) as f64 / 255.0;
    let g = hex_byte(h, 2) as f64 / 255.0;
    let b = hex_byte(h, 4) as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d < 0.08 || max < 0.1 || min > 0.9 {
        return Some("slate".to_string());
    }
    let hue = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let hue = if hue < 0.0 { hue + 360.0 } else { hue };
    // legacy maps hue ranges to GTK accent names; port below
    let name = match hue {
        h if h < 15.0 || h >= 345.0 => "red",
        h if h < 45.0 => "orange",
        h if h < 70.0 => "yellow",
        h if h < 165.0 => "green",
        h if h < 195.0 => "teal",
        h if h < 250.0 => "blue",
        h if h < 300.0 => "purple",
        _ => "pink",
    };
    Some(name.to_string())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_ok_requires_core_colours() {
        let mut v = serde_json::json!({
            "background": "#000000", "surface": "#111111", "surfaceContainer": "#222222",
            "surfaceVariant": "#333333", "foreground": "#eeeeee", "fgLight": "#dddddd",
            "bgDark": "#000000", "bgLight": "#222222", "muted": "#aaaaaa", "outline": "#555555",
            "primary": "#888888", "accent": "#999999", "accentAlt": "#888888", "secondary": "#777777",
            "tertiary": "#666666", "success": "#00ff00", "yellow": "#ffff00", "danger": "#ff0000", "error": "#ff0000"
        });
        assert!(palette_ok_value(&v));
        v["background"] = serde_json::json!("null");
        assert!(!palette_ok_value(&v));
        v.as_object_mut().unwrap().remove("primary");
        assert!(!palette_ok_value(&v));
    }

    #[test]
    fn contrast_and_adjust() {
        assert!(contrast("#000000", "#ffffff").unwrap() > 20.0);
        assert!(contrast("#777777", "#777777").unwrap() < 2.0);
        let fixed = adjust_contrast("#777777", "#777777", 4.5).expect("adjustable");
        assert!(contrast(&fixed, "#777777").unwrap() >= 4.5 - 1e-9, "fixed={fixed}");
        assert_eq!(adjust_contrast("#000000", "#ffffff", 4.5), Some("#000000".to_string()));
        assert_eq!(adjust_contrast("not-a-colour", "#ffffff", 4.5), None);
    }
}

