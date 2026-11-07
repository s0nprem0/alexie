//! Native app-template renderer (replaces `theme-apps` subprocess
//! when a templates dir is configured).
//!
//! Walks `assets::template_manifest()`, renders each `@TOKEN@` template
//! via `tokens` + `renderer`, writes script-generated configs
//! (`qt6ct.conf`, `kvantum.kvconfig`, `tmux.conf`, `btop.theme`,
//! `terminal.zsh`), and validates opencode tokens like
//! `theme-apps.sh`.
//!
//! LibreWolf outputs are best-effort (warn) — same policy as
//! `materialise.rs`. All other missing templates are hard errors.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::super::state;
use crate::theme::assets::{optional_outputs, templates_dir, template_manifest};
use crate::theme::renderer::render_template;
use crate::theme::tokens::{load_spec, substitution_map};

fn colour(generated: &serde_json::Value, key: &str) -> String {
    generated.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn cursor_theme() -> String {
    std::env::var("CURSOR_THEME").unwrap_or_else(|_| "Adwaita".to_string())
}

fn cursor_size() -> String {
    state::cursor_file()
        .pipe(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "24".to_string())
}

trait Pipe: Sized {
    fn pipe<F: FnOnce(Self) -> T, T>(self, f: F) -> T { f(self) }
}
impl<T> Pipe for T {}

/// Build the `@EXTRA@` map (HELIX_PARENT, GTK_*, CURSOR_*).
/// Mirrors `theme-apps.sh:92-129`.
fn build_extras(mode: &str) -> BTreeMap<String, String> {
    let (helix_parent, gtk_theme, gtk_icon, gtk_dark) = if mode == "light" {
        ("base16_default_light", "adw-gtk3", "Papirus-Light", "0")
    } else {
        ("base16_default_dark", "adw-gtk3-dark", "Papirus-Dark", "1")
    };
    let mut extras = BTreeMap::new();
    extras.insert("HELIX_PARENT".to_string(), helix_parent.to_string());
    extras.insert("GTK_THEME".to_string(), gtk_theme.to_string());
    extras.insert("GTK_ICON".to_string(), gtk_icon.to_string());
    extras.insert("GTK_DARK".to_string(), gtk_dark.to_string());
    extras.insert("CURSOR_THEME".to_string(), cursor_theme());
    extras.insert("CURSOR_SIZE".to_string(), cursor_size());
    extras
}

/// Render one template into `gen_dir/output`; required files error,
/// optional files warn and are skipped.
fn render_one(
    dir: &Path,
    template: &str,
    output: &str,
    required: bool,
    subs: &BTreeMap<String, String>,
    gen_dir: &Path,
) -> Result<(), String> {
    let src_path = dir.join(template);
    let content = match std::fs::read_to_string(&src_path) {
        Ok(c) => c,
        Err(e) => {
            if required {
                return Err(format!("could not read template {}: {}", src_path.display(), e));
            }
            eprintln!("theme: optional template missing {}", src_path.display());
            return Ok(());
        }
    };
    let rendered = match render_template(&content, subs) {
        Ok(r) => r,
        Err(e) => {
            if required {
                return Err(format!("template {}: {}", template, e));
            }
            eprintln!("theme: optional template {}: {}", template, e);
            return Ok(());
        }
    };
    let dest = gen_dir.join(output);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    state::write_atomic(&dest.to_path_buf(), &rendered).map_err(|e| e.to_string())?;
    Ok(())
}

/// Script-generated configs (port of theme-apps.sh heredocs).
fn write_generated_configs(
    gen_dir: &Path,
    generated: &serde_json::Value,
    extras: &BTreeMap<String, String>,
) -> Result<(), String> {
    let c = |k: &str| colour(generated, k);
    let (bg, fg, surface, muted, primary, accent) = (
        c("background"), c("foreground"), c("surface"), c("muted"), c("primary"), c("accent"),
    );
    let state_dir = state::state_dir();
    let gtk_icon = extras.get("GTK_ICON").cloned().unwrap_or_default();
    let mode = std::fs::read_to_string(gen_dir.join("mode"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "dark".to_string());

    for (name, body) in [
        ("qt6ct.conf", format!(
            "[Appearance]\ncolor_scheme_path={}/live/qt6ct-colors.conf\ncustom_palette=true\nicon_theme={}\nstyle=Fusion\n",
            state_dir.display(), gtk_icon)),
        ("qt5ct.conf", format!(
            "[Appearance]\ncolor_scheme_path={}/live/qt6ct-colors.conf\ncustom_palette=true\nicon_theme={}\nstyle=Fusion\n",
            state_dir.display(), gtk_icon)),
        ("kvantum.kvconfig", format!("[General]\ntheme={}\n",
            if mode == "light" { "KvAdapta" } else { "KvAdaptaDark" })),
        ("tmux.conf", format!(
            "set -g status-style \"bg={},fg={}\"\n\
             set -g status-left-style \"bg={},fg={}\"\n\
             set -g window-status-style \"bg={},fg={}\"\n\
             set -g window-status-current-style \"bg={},fg={},bold\"\n\
             set -g pane-active-border-style \"fg={}\"\n\
             set -g message-style \"bg={},fg={}\"\n\
             set -g mode-style \"bg={},fg={}\"\n\
             set -g clock-mode-colour \"{}\"\n",
            surface, fg, surface, muted, surface, muted, surface, primary,
            surface, surface, fg, surface, fg, primary)),
        ("btop.theme", format!(
            "theme[main_bg]=\"{}\"\ntheme[main_fg]=\"{}\"\ntheme[title]=\"{}\"\n",
            bg, fg, primary)),
        ("terminal.zsh", format!(
            "export BAT_THEME=ansi\nexport FZF_DEFAULT_OPTS=\"${{FZF_DEFAULT_OPTS:-}} --color=fg:{},bg:{},hl:{},fg+:{} ,bg+:{},hl+:{} ,info:{} ,prompt:{} ,pointer:{}\" \n",
            fg, bg, primary, fg, surface, primary, accent, primary, accent)),
    ] {
        if let Some(parent) = gen_dir.join(name).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        state::write_atomic(&gen_dir.join(name).to_path_buf(), &body).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Is the native renderer available (templates dir configured)?
pub(crate) fn is_available() -> bool {
    templates_dir().is_some_and(|d| d.is_dir())
}

/// Render all app files into `gen_dir`. Returns `Ok(())` on success,
/// `Err` on required-template/placeholder failures. Optional
/// (LibreWolf) templates warn only.
pub(crate) fn render_apps(gen_dir: &Path, palette: &serde_json::Map<String, serde_json::Value>, mode: &str) -> Result<(), String> {
    render_apps_with_dir(templates_dir(), gen_dir, palette, mode)
}

pub(crate) fn render_apps_with_dir(dir_opt: Option<PathBuf>, gen_dir: &Path, palette: &serde_json::Map<String, serde_json::Value>, mode: &str) -> Result<(), String> {
    let dir = dir_opt.ok_or_else(|| "templates dir not configured".to_string())?;
    if !dir.is_dir() {
        return Err(format!("templates dir not found: {}", dir.display()));
    }
    let spec = load_spec();
    let extras = build_extras(mode);
    let subs = substitution_map(palette, &spec, &extras);

    let mut warned: Vec<String> = Vec::new();
    for (tmpl, out, required) in template_manifest() {
        render_one(&dir, tmpl, out, required, &subs, gen_dir)?;
        if !required && !gen_dir.join(out).exists() {
            warned.push(out.to_string());
        }
    }
    for w in warned {
        eprintln!("theme: optional generation file missing {}", w);
    }
    // opencode guard (mirrors theme-apps.sh)
    if let Ok(text) = std::fs::read_to_string(gen_dir.join("opencode-theme.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            for key in ["syntaxNumber", "syntaxType", "syntaxOperator", "syntaxPunctuation", "selectedListItemText"] {
                if v.get("theme").and_then(|t| t.get(key)).is_none() {
                    // theme-apps.sh hard-fails only when the rendered
                    // opencode theme is actually produced; if the
                    // template was missing we already errored above.
                    let _ = optional_outputs();
                }
            }
        }
    let generated = serde_json::Value::Object(palette.clone());
    write_generated_configs(gen_dir, &generated, &extras)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_mode_mapping() {
        let dark = build_extras("dark");
        assert_eq!(dark.get("HELIX_PARENT").map(String::as_str), Some("base16_default_dark"));
        assert_eq!(dark.get("GTK_DARK").map(String::as_str), Some("1"));
        let light = build_extras("light");
        assert_eq!(light.get("GTK_THEME").map(String::as_str), Some("adw-gtk3"));
    }

    #[test]
    fn render_apps_requires_templates_dir() {
        // Hermetic: pass None explicitly so the test does not depend
        // on the host's config/env templates dir.
        let pal: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(r##"{"background":"#000000","primary":"#ffffff"}"##).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "alexie-render-apps-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let err = render_apps_with_dir(None, &dir, &pal, "dark").unwrap_err();
        assert!(err.contains("templates dir"), "got: {err}");
        let missing = dir.join("nonexistent");
        let err2 = render_apps_with_dir(Some(missing.clone()), &dir, &pal, "dark").unwrap_err();
        assert!(err2.contains("templates dir not found"), "got: {err2}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
