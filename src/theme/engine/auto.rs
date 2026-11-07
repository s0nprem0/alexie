use super::super::state::{self};
use super::apps::set_theme;
use super::generate::wallpaper_theme;
use crate::proc::run;
use std::env;
use std::path::Path;

// ---------- auto (port of auto_theme) ----------

fn auto_minutes(s: &str) -> i64 {
    let mut parts = s.split(':');
    let hour: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let minute: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    hour * 60 + minute
}

pub(crate) fn auto_mode(light: &str, dark: &str) -> String {
    auto_mode_at(light, dark, &{
        let (code, out, _) = run("date", &["+%H:%M"]);
        if code == 0 {
            out.trim().to_string()
        } else {
            "12:00".to_string()
        }
    })
}

fn auto_mode_at(light: &str, dark: &str, now: &str) -> String {
    let light_at = auto_minutes(light);
    let dark_at = auto_minutes(dark);
    let now_at = auto_minutes(now);
    if light_at <= dark_at {
        if now_at >= light_at && now_at < dark_at {
            "light".into()
        } else {
            "dark".into()
        }
    } else if now_at >= light_at || now_at < dark_at {
        "light".into()
    } else {
        "dark".into()
    }
}

/// Find the paired/family/mode theme for auto switching, in order:
/// explicit _pair, then same family + target mode, then any target
/// mode theme (legacy falls back with a stderr note).
pub(crate) fn find_auto_pair(
    palettes: &serde_json::Value,
    current: &str,
    target_mode: &str,
) -> Option<String> {
    let obj = palettes.as_object()?;
    let cur_entry = obj.get(current)?;
    let pair = cur_entry
        .get("_pair")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    if let Some(p) = pair
        && obj.contains_key(p)
    {
        return Some(p.to_string());
    }
    let family = cur_entry
        .get("_family")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // same family + target mode, sorted for determinism
    let mut candidates: Vec<&String> = obj.keys().filter(|k| *k != current).collect();
    candidates.sort();
    for k in &candidates {
        let e = &obj[*k];
        if e.get("_mode").and_then(|v| v.as_str()) == Some(target_mode)
            && e.get("_family").and_then(|v| v.as_str()).unwrap_or("") == family
        {
            return Some((*k).clone());
        }
    }
    for k in &candidates {
        let e = &obj[*k];
        if e.get("_mode").and_then(|v| v.as_str()) == Some(target_mode) {
            return Some((*k).clone());
        }
    }
    None
}

fn config_auto(light_key: &str, dark_key: &str) -> (String, String, bool) {
    let light = env::var("AUTO_LIGHT")
        .unwrap_or_else(|_| crate::config::get_str(&["theme", "auto", "light"], "07:00"));
    let dark = env::var("AUTO_DARK")
        .unwrap_or_else(|_| crate::config::get_str(&["theme", "auto", "dark"], "19:00"));
    let enabled = env::var("AUTO_SCHEDULE")
        .map(|v| v == "true")
        .unwrap_or_else(|_| {
            crate::config::get_bool(&["theme", "auto", "schedule", "enabled"], false)
        });
    let _ = (light_key, dark_key);
    (light, dark, enabled)
}

pub(crate) fn apply_auto() -> String {
    let _ = state::state_ensure();
    let (light, dark, enabled) = config_auto("light", "dark");
    if !enabled {
        return "OK".to_string();
    }
    let state = state::read_state_json();
    if state
        .as_ref()
        .and_then(|s| s.get("pinned"))
        .and_then(|p| p.as_bool())
        .unwrap_or(false)
    {
        return "OK".to_string();
    }
    let target = auto_mode(&light, &dark);
    if state::current_mode() == target {
        return "OK".to_string();
    }
    let name = state::current_theme();
    if name == "wallpaper" {
        // regenerate from the wallpaper fallback path, like legacy
        let source = std::fs::read_to_string(state::state_dir().join("wallpapers.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("themes")
                    .and_then(|t| t.get("wallpaper"))
                    .and_then(|t| t.get("fallback"))
                    .and_then(|f| f.get("path"))
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        if !source.is_empty() && Path::new(&source).is_file() {
            return wallpaper_theme(&source, &target);
        }
        return "OK".to_string();
    }
    if let Some(pair) = find_auto_pair(&state::palettes(), &name, &target)
        && state::has_theme(&pair)
    {
        return set_theme(&pair);
    }
    "OK".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_mode_schedule() {
        assert_eq!(auto_mode_at("07:00", "19:00", "12:00"), "light");
        assert_eq!(auto_mode_at("07:00", "19:00", "22:00"), "dark");
        assert_eq!(auto_mode_at("07:00", "19:00", "06:00"), "dark");
        assert_eq!(auto_mode_at("19:00", "07:00", "22:00"), "light");
        assert_eq!(auto_mode_at("19:00", "07:00", "12:00"), "dark");
    }

    #[test]
    fn find_auto_pair_prefers_pair_then_family() {
        let pals = serde_json::json!({
            "cur-dark": {"_mode": "dark", "_family": "fam", "_pair": "paired-light"},
            "paired-light": {"_mode": "light", "_family": "fam"},
            "other-light": {"_mode": "light", "_family": "other"},
            "other-dark": {"_mode": "dark", "_family": "other"}
        });
        assert_eq!(
            find_auto_pair(&pals, "cur-dark", "light"),
            Some("paired-light".to_string())
        );
        let pals2 = serde_json::json!({
            "cur-dark": {"_mode": "dark", "_family": "fam"},
            "fam-light": {"_mode": "light", "_family": "fam"},
            "other-light": {"_mode": "light", "_family": "other"}
        });
        assert_eq!(
            find_auto_pair(&pals2, "cur-dark", "light"),
            Some("fam-light".to_string())
        );
    }
}
