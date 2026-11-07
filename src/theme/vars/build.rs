//! Colour-var building: hex maths, palette to vars, cache,
//! and vars generation from a preset source.

use super::super::palette::{get_matugen_palette, home_dir, load_preset_palette};
use std::path::PathBuf;

pub(crate) fn hex_to_rgb_tuple(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        Some((r, g, b))
    } else {
        None
    }
}

pub(crate) fn interpolate_color(c1: &str, c2: &str, factor: f64) -> String {
    let rgb1 = hex_to_rgb_tuple(c1).unwrap_or((0, 0, 0));
    let rgb2 = hex_to_rgb_tuple(c2).unwrap_or((255, 255, 255));
    let r = ((rgb1.0 as f64) * (1.0 - factor) + (rgb2.0 as f64) * factor).round() as u8;
    let g = ((rgb1.1 as f64) * (1.0 - factor) + (rgb2.1 as f64) * factor).round() as u8;
    let b = ((rgb1.2 as f64) * (1.0 - factor) + (rgb2.2 as f64) * factor).round() as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

pub(crate) fn build_vars(
    palette: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let mut vars = std::collections::HashMap::new();
    let bg = palette
        .get("surface")
        .cloned()
        .unwrap_or_else(|| "#1d2021".to_string());
    let bg_dark = palette
        .get("surface_container")
        .cloned()
        .unwrap_or_else(|| "#282828".to_string());
    let bg_light = palette
        .get("surface_variant")
        .cloned()
        .unwrap_or_else(|| "#3c3836".to_string());
    let fg = palette
        .get("on_surface")
        .cloned()
        .unwrap_or_else(|| "#ebdbb2".to_string());
    let fg_light = palette
        .get("on_surface_variant")
        .cloned()
        .unwrap_or_else(|| "#d5c4a1".to_string());
    let accent = palette
        .get("primary")
        .cloned()
        .unwrap_or_else(|| "#a9b665".to_string());
    let secondary = palette
        .get("secondary")
        .cloned()
        .unwrap_or_else(|| "#7daea3".to_string());
    let tertiary = palette
        .get("tertiary")
        .cloned()
        .unwrap_or_else(|| "#d8a657".to_string());
    let error = palette
        .get("error")
        .cloned()
        .unwrap_or_else(|| "#cc241d".to_string());

    let (bg_r, bg_g, bg_b) = hex_to_rgb_tuple(&bg).unwrap_or((0, 0, 0));
    let (fg_r, fg_g, fg_b) = hex_to_rgb_tuple(&fg).unwrap_or((255, 255, 255));
    let bg_brightness = 0.299 * (bg_r as f64) + 0.587 * (bg_g as f64) + 0.114 * (bg_b as f64);
    let fg_brightness = 0.299 * (fg_r as f64) + 0.587 * (fg_g as f64) + 0.114 * (fg_b as f64);
    let is_light = bg_brightness > fg_brightness;

    vars.insert("is_light".to_string(), is_light.to_string());
    vars.insert("bg".to_string(), bg.clone());
    vars.insert("bg_hex".to_string(), bg.trim_start_matches('#').to_string());
    vars.insert("bg_dark".to_string(), bg_dark.clone());
    vars.insert(
        "bg_dark_hex".to_string(),
        bg_dark.trim_start_matches('#').to_string(),
    );
    vars.insert("bg_light".to_string(), bg_light.clone());
    vars.insert(
        "bg_light_hex".to_string(),
        bg_light.trim_start_matches('#').to_string(),
    );
    vars.insert("fg".to_string(), fg.clone());
    vars.insert("fg_hex".to_string(), fg.trim_start_matches('#').to_string());
    vars.insert("fg_light".to_string(), fg_light.clone());
    vars.insert(
        "fg_light_hex".to_string(),
        fg_light.trim_start_matches('#').to_string(),
    );
    vars.insert("accent".to_string(), accent.clone());
    vars.insert(
        "accent_hex".to_string(),
        accent.trim_start_matches('#').to_string(),
    );
    vars.insert("secondary".to_string(), secondary.clone());
    vars.insert(
        "secondary_hex".to_string(),
        secondary.trim_start_matches('#').to_string(),
    );
    vars.insert("tertiary".to_string(), tertiary.clone());
    vars.insert(
        "tertiary_hex".to_string(),
        tertiary.trim_start_matches('#').to_string(),
    );
    vars.insert("error".to_string(), error.clone());
    vars.insert(
        "error_hex".to_string(),
        error.trim_start_matches('#').to_string(),
    );

    let hex_to_rgb = |hex: &str| -> String {
        let h = hex.trim_start_matches('#');
        if h.len() == 6 {
            let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
            format!("{},{},{}", r, g, b)
        } else {
            "0,0,0".to_string()
        }
    };
    vars.insert("bg_rgb".to_string(), hex_to_rgb(&bg));
    vars.insert("bg_dark_rgb".to_string(), hex_to_rgb(&bg_dark));
    vars.insert("bg_light_rgb".to_string(), hex_to_rgb(&bg_light));
    vars.insert("fg_rgb".to_string(), hex_to_rgb(&fg));
    vars.insert("fg_light_rgb".to_string(), hex_to_rgb(&fg_light));
    vars.insert("accent_rgb".to_string(), hex_to_rgb(&accent));
    vars.insert("secondary_rgb".to_string(), hex_to_rgb(&secondary));
    vars.insert("error_rgb".to_string(), hex_to_rgb(&error));

    for i in 0..8 {
        let factor = i as f64 / 7.0;
        let color = interpolate_color(&accent, &tertiary, factor);
        vars.insert(format!("cava_color_{}", i + 1), color);
    }

    vars.insert("home".to_string(), home_dir().to_string_lossy().to_string());
    vars
}

pub(crate) fn vars_cache_path() -> PathBuf {
    home_dir()
        .join(".cache")
        .join("alexie")
        .join("theme_vars.json")
}

pub(crate) fn load_cached_vars() -> Option<std::collections::HashMap<String, String>> {
    let p = vars_cache_path();
    let s = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}

pub(crate) fn save_cached_vars(vars: &std::collections::HashMap<String, String>) {
    let p = vars_cache_path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(vars) {
        let _ = std::fs::write(p, json);
    }
}

pub(crate) fn generate_vars(
    source: &str,
    value: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    let palette = if source == "preset" {
        load_preset_palette(value).ok_or_else(|| format!("unknown preset {}", value))?
    } else {
        // wallpaper
        let path = std::fs::canonicalize(value).map_err(|e| e.to_string())?;
        get_matugen_palette(&path, "auto").ok_or_else(|| "matugen failed".to_string())?
    };
    Ok(build_vars(&palette))
}
