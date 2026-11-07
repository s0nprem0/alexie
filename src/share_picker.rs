//! Portal share-picker: config + style for hyprland-preview-share-picker.

use crate::proc::find_in_path;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::PathBuf;

fn picker_bin() -> String {
    if let Ok(v) = env::var("SHARE_PICKER_BIN")
        && !v.is_empty() {
            return v;
        }
    let path = env::var("PATH").unwrap_or_default();
    find_in_path("hyprland-preview-share-picker", &path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "hyprland-preview-share-picker".to_string())
}

fn slurp_bin() -> String {
    if let Ok(v) = env::var("SLURP")
        && !v.is_empty() {
            return v;
        }
    let path = env::var("PATH").unwrap_or_default();
    find_in_path("slurp", &path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "slurp".to_string())
}

fn config_dir() -> PathBuf {
    let base = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.config", home)
    });
    PathBuf::from(base).join("hyprland-preview-share-picker")
}

pub(crate) fn style_path() -> PathBuf { config_dir().join("style.css") }
pub(crate) fn config_path() -> PathBuf { config_dir().join("config.yaml") }

pub(crate) fn xdph_path() -> PathBuf {
    let base = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.config", home)
    });
    PathBuf::from(base).join("hypr").join("xdph.conf")
}

pub(crate) fn render_config(style: &str, slurp: &str) -> String {
    format!(
        "stylesheets:\n  - \"{}\"\ndefault_page: windows\n\nwindow:\n  height: 500\n  width: 1000\n\nimage:\n  resize_size: 200\n  widget_size: 150\n\nwindows:\n  min_per_row: 3\n  max_per_row: 999\n  clicks: 2\n  spacing: 12\n\noutputs:\n  clicks: 2\n  spacing: 6\n  show_label: false\n  respect_output_scaling: true\n\nregion:\n  command: \"{} -f '%o@%x,%y,%w,%h'\"\n\nhide_token_restore: false\ndebug: false\n",
        style, slurp
    )
}

pub(crate) fn render_xdph(picker: &str) -> String {
    format!(
        "screencopy {{\n  custom_picker_binary = {}\n}}\n",
        picker
    )
}

#[derive(Serialize)]
pub struct Status {
    pub picker: String,
    pub slurp: String,
    pub config: String,
    pub style: String,
    pub xdph: String,
    pub enabled: bool,
}

pub(crate) fn status() -> Status {
    let picker = picker_bin();
    let enabled = fs::read_to_string(xdph_path())
        .map(|s| s.contains("custom_picker_binary"))
        .unwrap_or(false);
    Status {
        picker,
        slurp: slurp_bin(),
        config: config_path().to_string_lossy().to_string(),
        style: style_path().to_string_lossy().to_string(),
        xdph: xdph_path().to_string_lossy().to_string(),
        enabled,
    }
}

fn write_atomic(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

fn apply() -> String {
    let picker = picker_bin();
    let slurp = slurp_bin();
    let style = style_path().to_string_lossy().to_string();
    let config = render_config(&style, &slurp);
    if let Err(e) = write_atomic(&config_path(), &config) {
        return format!("ERR {}", e);
    }
    // Link style to the live theme when present.
    let live = crate::theme::state::live_link().join("share-picker.css");
    if live.is_file() {
        if let Some(parent) = style_path().parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::remove_file(style_path());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let _ = symlink(&live, style_path());
        }
    }
    if let Err(e) = write_atomic(&xdph_path(), &render_xdph(&picker)) {
        return format!("ERR {}", e);
    }
    "OK".to_string()
}

fn check() -> String {
    let picker_path = PathBuf::from(picker_bin());
    let mut out = String::new();
    out.push_str(&format!("picker: {} ({})\n", picker_bin(), if picker_path.exists() { "found" } else { "missing" }));
    out.push_str(&format!("config: {} ({})\n", config_path().display(), if config_path().is_file() { "found" } else { "missing" }));
    out.push_str(&format!("xdph: {} ({})\n", xdph_path().display(), if xdph_path().is_file() { "found" } else { "missing" }));
    out.push_str(&format!("style: {} ({})\n", style_path().display(), if style_path().exists() { "found" } else { "missing" }));
    out.trim_end().to_string()
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "-h" | "--help" | "help" => "share-picker — portal picker config\n\nUsage:\n  share-picker status [--json]\n  share-picker apply\n  share-picker check\n".to_string(),
        "status" => {
            let want_json = args.iter().any(|a| a == "--json");
            let s = status();
            if want_json {
                serde_json::to_string(&s).unwrap_or_else(|_| "{}".to_string())
            } else {
                format!("picker {} {}\nconfig {}\nstyle {}\nxdph {}", s.picker, if s.enabled { "enabled" } else { "disabled" }, s.config, s.style, s.xdph)
            }
        }
        "apply" => apply(),
        "check" => check(),
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_config_contains_paths() {
        let c = render_config("/tmp/style.css", "/nix/slurp");
        assert!(c.contains("/tmp/style.css"));
        assert!(c.contains("/nix/slurp"));
        assert!(c.contains("custom_picker") || c.contains("default_page"));
    }

    #[test]
    fn render_xdph_contains_picker() {
        assert!(render_xdph("/nix/picker").contains("/nix/picker"));
    }

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }
}
