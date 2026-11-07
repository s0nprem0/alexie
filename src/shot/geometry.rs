//! Geometry from hyprctl JSON, window slug, timestamp.
//! Pure helpers, testable without hyprctl/jq (except window_slug,
//! which fetches the class).

use super::hyprctl;

pub(crate) fn window_geometry_from_json(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let at = v.get("at")?.as_array()?;
    let size = v.get("size")?.as_array()?;
    if at.len() < 2 || size.len() < 2 {
        return None;
    }
    let x = at[0].as_i64()?;
    let y = at[1].as_i64()?;
    let w = size[0].as_i64()?;
    let h = size[1].as_i64()?;
    Some(format!("{x},{y} {w}x{h}"))
}

/// Geometry for the focused monitor, from hyprctl monitors JSON.
pub(crate) fn focused_monitor_geometry_from_json(json: &str) -> Option<String> {
    // Match legacy focused_monitor_geo() in pick.sh: hyprctl
    // width/height are physical pixels, so logical size is
    // width/scale, height/scale, swapped when transform is 1 or 3
    // (rotated). Without the scale division a scaled (e.g. 1.5x)
    // monitor captures far beyond its logical area.
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = v.as_array()?;
    let mon = arr
        .iter()
        .find(|m| m.get("focused").and_then(|f| f.as_bool()) == Some(true))?;
    let x = mon.get("x")?.as_i64()?;
    let y = mon.get("y")?.as_i64()?;
    let width = mon.get("width")?.as_f64()?;
    let height = mon.get("height")?.as_f64()?;
    let scale = mon.get("scale").and_then(|s| s.as_f64()).unwrap_or(1.0);
    if scale <= 0.0 {
        return None;
    }
    let mut w = (width / scale).round() as i64;
    let mut h = (height / scale).round() as i64;
    let transform = mon.get("transform").and_then(|t| t.as_i64()).unwrap_or(0);
    if transform == 1 || transform == 3 {
        std::mem::swap(&mut w, &mut h);
    }
    Some(format!("{x},{y} {w}x{h}"))
}

/// Window class from hyprctl activewindow JSON.
pub(crate) fn window_class_from_json(json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return String::new();
    };
    v.get("class")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn window_slug() -> String {
    let (code, out, _) = crate::proc::run(&hyprctl(), &["activewindow", "-j"]);
    if code != 0 {
        return String::new();
    }
    let class = window_class_from_json(&out);
    if class.is_empty() {
        return String::new();
    }
    slugify_class(&class)
}

pub(crate) fn slugify_class(class: &str) -> String {
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

pub(crate) fn chrono_like_stamp() -> String {
    // use date command
    let (code, out, _) = crate::proc::run("date", &["+%Y-%m-%d_%H-%M-%S"]);
    if code == 0 {
        out.trim().to_string()
    } else {
        "0".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_from_json() {
        assert_eq!(
            window_geometry_from_json(r#"{"at":[10,20],"size":[800,600]}"#),
            Some("10,20 800x600".to_string())
        );
        assert_eq!(
            window_geometry_from_json(r#"{"at":null,"size":null}"#),
            None
        );
        assert_eq!(window_geometry_from_json("not json"), None);
        assert_eq!(
            focused_monitor_geometry_from_json(
                r#"[{"name":"eDP-1","x":0,"y":0,"width":1920,"height":1080,"focused":false},{"name":"HDMI-A-1","x":1920,"y":0,"width":2560,"height":1440,"focused":true}]"#
            ),
            Some("1920,0 2560x1440".to_string())
        );
        assert_eq!(
            focused_monitor_geometry_from_json(
                r#"[{"name":"eDP-1","x":0,"y":0,"width":3840,"height":2160,"scale":2,"transform":1,"focused":true}]"#
            ),
            Some("0,0 1080x1920".to_string())
        );
        assert_eq!(focused_monitor_geometry_from_json("[]"), None);
        assert_eq!(
            window_class_from_json(r#"{"class":"kitty"}"#),
            "kitty".to_string()
        );
        assert_eq!(slugify_class("Firefox Developer"), "firefox-developer");
    }

    #[test]
    fn shell_quote_basic() {
        use crate::proc::shell_quote;
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
