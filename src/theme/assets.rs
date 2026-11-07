//! Theme asset resolution + template manifest (Phase 2).
//!
//! Mirrors `theming/default.nix:48-113` `@*_TEMPLATE@` wiring.
//! anixie owns the template files; alexie walks the manifest and
//! renders `@TOKEN@` templates into a generation dir.
//!
//! LibreWolf outputs are best-effort (warn, never fail a switch) so
//! the live bridge keeps working when browser templates are absent.

use std::path::PathBuf;

/// Resolve the templates dir: `ALEXIE_THEME_TEMPLATES` env →
/// `theme.templates_dir` config → `None` (callers fall back to
/// external `theme-apps` subprocess).
pub(crate) fn templates_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("ALEXIE_THEME_TEMPLATES")
        && !v.trim().is_empty()
    {
        return Some(PathBuf::from(v));
    }
    if let Ok(v) = std::env::var("THEME_TEMPLATES_DIR")
        && !v.trim().is_empty()
    {
        return Some(PathBuf::from(v));
    }
    let v = crate::config::get_str(&["theme", "templates_dir"], "");
    if v.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(v))
    }
}

/// (template filename, output filename, required)
/// Mirrors `theming/default.nix` manifest order. LibreWolf = optional.
pub(crate) fn template_manifest() -> Vec<(&'static str, &'static str, bool)> {
    vec![
        ("helix.toml", "helix.toml", true),
        ("yazi.toml", "yazi.toml", true),
        ("hyprlock.conf", "hyprlock.conf", true),
        ("qt6ct-colors.conf", "qt6ct-colors.conf", true),
        ("gtk-settings.ini", "gtk-3.0-settings.ini", true),
        ("gtk-settings.ini", "gtk-4.0-settings.ini", true),
        ("gtk-3.0.css", "gtk-3.0.css", true),
        ("gtk-4.0.css", "gtk-4.0.css", true),
        ("share-picker/style.css", "share-picker.css", true),
        ("discord.css", "discord.css", true),
        ("alacritty.toml", "alacritty.toml", true),
        ("ghostty", "ghostty", true),
        ("foot.ini", "foot.ini", true),
        ("vscode.json", "vscode.json", true),
        ("obsidian.css", "obsidian.css", true),
        (
            "librewolf-userChrome.css",
            "librewolf-userChrome.css",
            false,
        ),
        (
            "librewolf-userContent.css",
            "librewolf-userContent.css",
            false,
        ),
        ("mpv.conf", "mpv.conf", true),
        ("zathurarc", "zathurarc", true),
        ("eza-theme.yml", "eza-theme.yml", true),
        ("opencode-theme.json", "opencode-theme.json", true),
    ]
}

/// Required outputs (hard error if missing in a generation).
pub(crate) fn required_outputs() -> Vec<&'static str> {
    template_manifest()
        .into_iter()
        .filter(|(_, _, required)| *required)
        .map(|(_, out, _)| out)
        .collect()
}

/// Optional outputs (warn only — LibreWolf best-effort).
pub(crate) fn optional_outputs() -> Vec<&'static str> {
    template_manifest()
        .into_iter()
        .filter(|(_, _, required)| !*required)
        .map(|(_, out, _)| out)
        .collect()
}

pub(crate) fn renderer_id() -> String {
    // Cheap fingerprint for `theme apply` skip logic. Bump when the
    // manifest changes so existing generations are re-rendered.
    let m = template_manifest();
    let mut h = format!("v1:{}", m.len());
    for (tmpl, out, req) in &m {
        h.push_str(&format!(
            "|{}->{}{}",
            tmpl,
            out,
            if *req { '!' } else { '?' }
        ));
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_includes_librewolf_as_optional() {
        let m = template_manifest();
        let lw: Vec<_> = m
            .iter()
            .filter(|(t, _, _)| t.contains("librewolf"))
            .collect();
        assert_eq!(lw.len(), 2);
        assert!(lw.iter().all(|(_, _, req)| !*req));
    }

    #[test]
    fn required_excludes_librewolf() {
        let req = required_outputs();
        assert!(!req.iter().any(|o| o.contains("librewolf")));
        assert!(req.contains(&"helix.toml"));
        assert!(req.contains(&"opencode-theme.json"));
    }

    #[test]
    fn renderer_id_stable() {
        assert_eq!(renderer_id(), renderer_id());
        assert!(renderer_id().starts_with("v1:"));
    }
}
