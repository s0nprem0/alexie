//! `vars`/`render` commands and presets listing.

use super::super::palette::presets_dir;
use super::build::{generate_vars, load_cached_vars, save_cached_vars};
use std::collections::BTreeMap;
use std::fs;

/// Parse the arguments after `theme vars`.
///
/// `args` is the slice *after* the `vars` token, so:
/// - `[]` → no explicit source/value (use cache)
/// - `["preset", NAME]` / `["wallpaper", PATH]` → explicit
/// - `[VALUE]` → inferred: existing path = wallpaper, otherwise preset
///
/// Returns `Ok(None)` for the no-args case, `Ok(Some((source, value)))`
/// for an explicit/inferred choice, and `Err(usage)` for a malformed
/// invocation. The previous implementation indexed `args[1]`/`args[2]`
/// (as if `vars` itself were still in the slice), so explicit
/// `vars preset NAME` / `vars wallpaper PATH` were parsed by accident
/// via the path-exists fallback and `vars preset` with no name fell
/// through to the cache path instead of a usage error.
pub(crate) fn parse_vars_args(args: &[String]) -> Result<Option<(String, String)>, String> {
    const USAGE: &str = "ERR usage: theme vars [preset NAME | wallpaper PATH]";
    match args {
        [] => Ok(None),
        [kind, value] if kind == "preset" || kind == "wallpaper" => {
            Ok(Some((kind.clone(), value.clone())))
        }
        [kind] if kind == "preset" || kind == "wallpaper" => Err(USAGE.to_string()),
        [value] => {
            let source = if std::path::Path::new(value).exists() {
                "wallpaper"
            } else {
                "preset"
            };
            Ok(Some((source.to_string(), value.clone())))
        }
        _ => Err(USAGE.to_string()),
    }
}

pub(crate) fn theme_vars(args: &[String]) -> String {
    // theme vars [preset NAME | wallpaper PATH | PATH]
    let parsed = match parse_vars_args(args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some((source, value)) = parsed else {
        // No explicit choice: return cached vars if we have them.
        if let Some(v) = load_cached_vars() {
            return serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string());
        }
        return "ERR usage: theme vars [preset NAME | wallpaper PATH]".to_string();
    };
    if value.is_empty() {
        return "ERR usage: theme vars [preset NAME | wallpaper PATH]".to_string();
    }
    match generate_vars(&source, &value) {
        Ok(vars) => {
            save_cached_vars(&vars);
            serde_json::to_string(&vars).unwrap_or_else(|_| "{}".to_string())
        }
        Err(e) => format!("ERR {}", e),
    }
}

/// Pure template substitution, extracted so tests do not need the
/// vars cache on disk.
pub(crate) fn render_template(
    content: &str,
    vars: &std::collections::HashMap<String, String>,
) -> String {
    let mut rendered = content.to_string();
    for (k, v) in vars {
        let placeholder = format!("{{{{{}}}}}", k);
        rendered = rendered.replace(&placeholder, v);
    }
    rendered
}

pub(crate) fn theme_render(args: &[String]) -> String {
    // `args` is the slice after `render`, so exactly [TEMPLATE, OUTPUT].
    // The old check (`args.len() < 3` with `args[1]`/`args[2]`) made
    // `theme render T O` — a 2-element slice — always hit the usage error.
    if args.len() != 2 {
        return "ERR usage: theme render TEMPLATE OUTPUT".to_string();
    }
    let tmpl = &args[0];
    let out = &args[1];
    let vars = load_cached_vars().unwrap_or_default();
    let content = match std::fs::read_to_string(tmpl) {
        Ok(c) => c,
        Err(e) => return format!("ERR read template: {}", e),
    };
    let rendered = render_template(&content, &vars);
    if let Some(parent) = std::path::Path::new(out).parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(out, rendered) {
        Ok(_) => format!("rendered {}", out),
        Err(e) => format!("ERR write: {}", e),
    }
}

pub(crate) fn presets() -> String {
    let dir = presets_dir();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    if let Ok(read) = fs::read_dir(&dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // toml 1.x parses documents into a Table, not a bare Value.
            let value: toml::Table = match toml::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let colors_table = match value.get("colors").and_then(|v| v.as_table()) {
                Some(t) => t,
                None => continue,
            };
            let mut colors: BTreeMap<String, String> = BTreeMap::new();
            for (k, v) in colors_table {
                if let Some(s) = v.as_str() {
                    colors.insert(k.clone(), s.to_string());
                }
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            entries.push(serde_json::json!({
                "name": stem,
                "path": path.to_string_lossy(),
                "colors": colors,
            }));
        }
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn parse_vars_args_explicit() {
        assert_eq!(parse_vars_args(&[]), Ok(None));
        assert_eq!(
            parse_vars_args(&s(&["preset", "gruvbox-dark"])),
            Ok(Some(("preset".to_string(), "gruvbox-dark".to_string())))
        );
        assert_eq!(
            parse_vars_args(&s(&["wallpaper", "/tmp/wp.png"])),
            Ok(Some(("wallpaper".to_string(), "/tmp/wp.png".to_string())))
        );
    }

    #[test]
    fn parse_vars_args_inferred_single() {
        assert_eq!(
            parse_vars_args(&s(&["gruvbox-dark"])),
            Ok(Some(("preset".to_string(), "gruvbox-dark".to_string())))
        );
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        assert_eq!(
            parse_vars_args(&s(&[&dir])),
            Ok(Some(("wallpaper".to_string(), dir.clone())))
        );
    }

    #[test]
    fn parse_vars_args_rejects_malformed() {
        for bad in [
            s(&["preset"]),
            s(&["wallpaper"]),
            s(&["preset", "a", "extra"]),
            s(&["a", "b", "c"]),
        ] {
            let err = parse_vars_args(&bad).expect_err("malformed vars args");
            assert!(
                err.starts_with("ERR usage: theme vars"),
                "got: {err} for {bad:?}"
            );
        }
    }

    #[test]
    fn theme_vars_preset_missing_name_is_usage() {
        let out = theme_vars(&s(&["preset"]));
        assert!(out.starts_with("ERR usage: theme vars"), "got: {out}");
        let out = theme_vars(&s(&["wallpaper"]));
        assert!(out.starts_with("ERR usage: theme vars"), "got: {out}");
        let out = theme_vars(&s(&["a", "b", "c"]));
        assert!(out.starts_with("ERR usage: theme vars"), "got: {out}");
    }

    #[test]
    fn theme_vars_unknown_preset() {
        let out = theme_vars(&s(&["preset", "no-such-preset-xyz"]));
        assert!(out.starts_with("ERR unknown preset"), "got: {out}");
    }

    #[test]
    fn render_template_substitutes() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("bg".to_string(), "#112233".to_string());
        vars.insert("fg".to_string(), "#ddeeff".to_string());
        let out = render_template("bg={{bg}} fg={{fg}} other={{missing}}", &vars);
        assert_eq!(out, "bg=#112233 fg=#ddeeff other={{missing}}");
    }

    #[test]
    fn theme_render_usage() {
        for bad in [
            s(&["render"]),
            s(&["render", "only"]),
            s(&["render", "a", "b", "c"]),
        ] {
            // theme_render is called with args after `render`
            let args_after_render = &bad[1..];
            let out = theme_render(args_after_render);
            assert!(
                out.starts_with("ERR usage: theme render"),
                "got: {out} for {bad:?}"
            );
        }
    }

    #[test]
    fn theme_render_writes_output() {
        let dir = std::env::temp_dir().join(format!(
            "alexie-render-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tmpl = dir.join("tmpl.txt");
        let out = dir.join("sub").join("out.txt");
        std::fs::write(&tmpl, "hello {{bg}}\n").unwrap();
        let res = theme_render(&[
            tmpl.to_string_lossy().to_string(),
            out.to_string_lossy().to_string(),
        ]);
        assert!(res.starts_with("rendered "), "got: {res}");
        assert!(out.is_file(), "output file created");
        let body = std::fs::read_to_string(&out).unwrap();
        assert!(body.starts_with("hello "), "got: {body}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
