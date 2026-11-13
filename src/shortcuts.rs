//! Keyboard shortcuts: read Hyprland's described binds (`hyprctl binds -j`)
//! and present them through the picker. Native port of anixie's
//! `shortcuts.sh` (no `jq` dependency).

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Bind {
    description: String,
    submap: String,
    modmask: i64,
    key: String,
}

pub fn handle(args: &[String]) -> String {
    if args.iter().any(|a| matches!(a.as_str(), "-h" | "--help" | "help")) {
        return "usage: shortcuts\n\n  Show Hyprland's described keybindings in a searchable picker.".to_string();
    }

    let hyprctl = crate::proc::tool("HYPRCTL", "hyprctl");
    let (code, out, err) = crate::proc::run_timeout(&hyprctl, &["binds", "-j"], std::time::Duration::from_secs(5));
    if code != 0 {
        let msg = err.trim();
        if msg.is_empty() {
            return format!("ERR hyprctl binds failed (exit {})", code);
        }
        return format!("ERR {}", msg);
    }

    let value: Value = match serde_json::from_str(&out) {
        Ok(v) => v,
        Err(e) => return format!("ERR invalid binds JSON: {}", e),
    };
    let arr = match value.as_array() {
        Some(a) => a,
        None => return "ERR expected a JSON array of binds".to_string(),
    };

    let mut binds: Vec<Bind> = arr.iter().filter_map(parse_bind).collect();

    binds.sort_by(|a, b| {
        a.description
            .cmp(&b.description)
            .then_with(|| a.submap.cmp(&b.submap))
            .then_with(|| a.key.cmp(&b.key))
    });

    if binds.is_empty() {
        return "ERR no described shortcuts".to_string();
    }

    let mut tsv = String::new();
    for b in &binds {
        tsv.push_str(&bind_line(b));
        tsv.push('\n');
    }

    let pick_args: Vec<String> = [
        "--title",
        "Keyboard shortcuts",
        "--placeholder",
        "Search shortcuts",
        "--on-select",
        "true",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    crate::pick::handle_with_stdin(&pick_args, &tsv)
}

fn parse_bind(v: &Value) -> Option<Bind> {
    let obj = v.as_object()?;
    let has_description = obj
        .get("has_description")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !has_description {
        return None;
    }
    let description_raw = obj.get("description").and_then(Value::as_str)?;
    if description_raw.is_empty() {
        return None;
    }
    let modmask = obj.get("modmask").and_then(Value::as_i64)?;
    let submap_raw = obj
        .get("submap")
        .and_then(|x| x.as_str().map(str::to_string))
        .unwrap_or_default();
    let key_raw = match obj.get("key") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };

    Some(Bind {
        description: sanitize(description_raw),
        submap: sanitize(&submap_raw),
        modmask,
        key: sanitize(&key_raw),
    })
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

fn mod_string(modmask: i64) -> String {
    let mut mods: Vec<&str> = Vec::new();
    if modmask & 64 != 0 {
        mods.push("SUPER");
    }
    if modmask & 4 != 0 {
        mods.push("CTRL");
    }
    if modmask & 8 != 0 {
        mods.push("ALT");
    }
    if modmask & 1 != 0 {
        mods.push("SHIFT");
    }
    if modmask & 2 != 0 {
        mods.push("CAPS");
    }
    if modmask & 16 != 0 {
        mods.push("MOD2");
    }
    if modmask & 32 != 0 {
        mods.push("MOD3");
    }
    if modmask & 128 != 0 {
        mods.push("MOD5");
    }
    mods.join(" + ")
}

fn bind_line(b: &Bind) -> String {
    let key_display = if b.key.is_empty() {
        "(none)".to_string()
    } else {
        b.key.clone()
    };
    let mods = mod_string(b.modmask);
    let combo = if mods.is_empty() {
        key_display
    } else {
        format!("{} + {}", mods, key_display)
    };
    let display = if b.submap.is_empty() {
        combo
    } else {
        format!("Submap: {} · {}", b.submap, combo)
    };
    format!("{}\t{}\t\t{}\t", b.description, b.description, display)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_string_super_ctrl() {
        assert_eq!(mod_string(68), "SUPER + CTRL");
    }

    #[test]
    fn mod_string_empty() {
        assert_eq!(mod_string(0), "");
    }

    #[test]
    fn bind_line_basic() {
        let b = Bind {
            description: "Open terminal".to_string(),
            submap: String::new(),
            modmask: 68,
            key: "Return".to_string(),
        };
        let line = bind_line(&b);
        assert_eq!(line, "Open terminal\tOpen terminal\t\tSUPER + CTRL + Return\t");
    }

    #[test]
    fn parse_bind_filters_invalid() {
        let v = serde_json::json!({
            "has_description": true,
            "description": "desc",
            "modmask": 64,
            "key": "a",
            "submap": ""
        });
        assert!(parse_bind(&v).is_some());

        let no_desc = serde_json::json!({"has_description": false, "description": "x", "modmask": 0});
        assert!(parse_bind(&no_desc).is_none());

        let empty_desc = serde_json::json!({"has_description": true, "description": "", "modmask": 0});
        assert!(parse_bind(&empty_desc).is_none());

        let no_modmask = serde_json::json!({"has_description": true, "description": "x"});
        assert!(parse_bind(&no_modmask).is_none());
    }

    #[test]
    fn handle_help() {
        let out = handle(&["--help".to_string()]);
        assert!(out.contains("usage: shortcuts"));
    }

    #[test]
    fn handle_no_hyprctl_returns_err() {
        unsafe { std::env::set_var("HYPRCTL", "/nonexistent/hyprctl"); }
        let out = handle(&[]);
        unsafe { std::env::remove_var("HYPRCTL"); }
        assert!(out.starts_with("ERR"), "got: {out}");
    }
}
