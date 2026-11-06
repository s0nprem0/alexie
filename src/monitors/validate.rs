pub(crate) fn transform_index(name: &str) -> Option<usize> {
    let names = ["normal","90","180","270","flipped","flipped-90","flipped-180","flipped-270"];
    names.iter().position(|&n| n == name)
}


pub(crate) fn parse_transform(v: &str) -> Option<usize> {
    if let Some(i) = transform_index(v) { return Some(i); }
    if let Ok(i) = v.parse::<usize>() {
        if i < 8 { return Some(i); }
    }
    None
}

pub(crate) fn valid_int(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

pub(crate) fn valid_mode(v: &str) -> bool {
    if matches!(v, "preferred" | "highres" | "highrr" | "maxwidth") { return true; }
    let mut at = v.splitn(2, '@');
    let base = at.next().unwrap_or("");
    let rate = at.next();
    if at.count() > 0 { return false; }
    let mut xb = base.splitn(2, 'x');
    let w = xb.next().unwrap_or("");
    let h = xb.next().unwrap_or("");
    if xb.count() > 0 { return false; }
    if w.is_empty() || !w.chars().all(|c| c.is_ascii_digit()) { return false; }
    if h.is_empty() || !h.chars().all(|c| c.is_ascii_digit()) { return false; }
    if let Some(r) = rate {
        if r.is_empty() || !r.chars().all(|c| c.is_ascii_digit() || c == '.') { return false; }
    }
    // ensure at most one '@'
    if v.matches('@').count() > 1 { return false; }
    true
}

pub(crate) fn valid_position(v: &str) -> bool {
    if matches!(v, "auto" | "auto-left" | "auto-right" | "auto-up" | "auto-down") { return true; }
    let mut xb = v.splitn(2, 'x');
    let x = xb.next().unwrap_or("");
    let y = xb.next().unwrap_or("");
    if xb.count() > 0 { return false; }
    if !v.contains('x') { return false; }
    valid_int(x) && valid_int(y)
}

/// Unified validation + normalization for a monitor setting (#2).
/// Returns the normalized value to pass to apply/save, or an error string.
pub(crate) fn validate_setting(key: &str, value: &str) -> Result<String, String> {
    match key {
        "scale" => {
            const LADDER: [&str; 7] = ["1", "1.25", "1.5", "1.6", "2", "3", "4"];
            if LADDER.contains(&value) {
                Ok(value.to_string())
            } else {
                Err("ERR scale must be one of: 1 1.25 1.5 1.6 2 3 4".to_string())
            }
        }
        "transform" => match parse_transform(value) {
            Some(i) => Ok(i.to_string()),
            None => Err("ERR transform must be one of: normal 90 180 270 flipped flipped-90 flipped-180 flipped-270".to_string()),
        },
        "position" => {
            if valid_position(value) {
                Ok(value.to_string())
            } else {
                Err("ERR position must be auto, auto-left/right/up/down, or XxY".to_string())
            }
        }
        "mode" => {
            let v = value.strip_suffix("Hz").unwrap_or(value).trim();
            if valid_mode(v) {
                Ok(v.to_string())
            } else {
                Err("ERR mode must be preferred, highres, highrr, or WxH[@R]".to_string())
            }
        }
        "sdrbrightness" => {
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit() || c == '.') {
                Ok(value.to_string())
            } else {
                Err("ERR sdrbrightness must be a number".to_string())
            }
        }
        "sdrsaturation" => {
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit() || c == '.') {
                Ok(value.to_string())
            } else {
                Err("ERR sdrsaturation must be a number".to_string())
            }
        }
        _ => Err("ERR unknown monitor setting".to_string()),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_index_known() {
        assert_eq!(transform_index("normal"), Some(0));
        assert_eq!(transform_index("90"), Some(1));
        assert_eq!(transform_index("flipped-270"), Some(7));
    }

    #[test]
    fn transform_index_unknown() {
        assert_eq!(transform_index("foo"), None);
        assert_eq!(transform_index(""), None);
    }

    #[test]
    fn valid_mode_accepts_core_forms() {
        assert!(valid_mode("preferred"));
        assert!(valid_mode("highres"));
        assert!(valid_mode("1920x1080"));
        assert!(valid_mode("1920x1080@60"));
        assert!(!valid_mode("1920x1080@60@144"));
    }

    #[test]
    fn valid_position_accepts_core_forms() {
        assert!(valid_position("auto"));
        assert!(valid_position("auto-left"));
        assert!(valid_position("1920x1080"));
        assert!(valid_position("-1920x0"));
        assert!(!valid_position("auto-middle"));
    }

    #[test]
    fn validate_setting_normalizes() {
        assert_eq!(validate_setting("scale", "1.5").unwrap(), "1.5");
        assert_eq!(validate_setting("transform", "flipped-90").unwrap(), "5");
        assert_eq!(validate_setting("mode", "1920x1080@60Hz").unwrap(), "1920x1080@60");
        assert_eq!(validate_setting("position", "auto-left").unwrap(), "auto-left");
        assert!(validate_setting("scale", "99").is_err());
        assert!(validate_setting("mode", "notamode").is_err());
    }

    #[test]
    fn parse_output_name_validation() {
        // valid names
        for n in ["eDP-1", "HDMI-A-1", "DP-2", "Virtual-1"] {
            assert!(n.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.'));
        }
        // invalid
        for n in ["", "eDP 1", "hdmi;rm", "a/b"] {
            let valid = !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.');
            assert!(!valid, "{} should be invalid", n);
        }
    }
}
