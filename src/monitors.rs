use crate::proc::{run, shell_quote};
use serde_json::Value;
use std::env;

fn state_dir() -> std::path::PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    std::path::PathBuf::from(state).join("anixie")
}

type MonitorState = (String, String, String, String, String, String, String, String);

fn state_file() -> std::path::PathBuf { state_dir().join("display.conf") }
fn lid_file() -> std::path::PathBuf { state_dir().join("lid") }

// Write `content` to `dest` atomically via a temp file in the same
// directory. `/tmp` may be on a different filesystem than the state dir,
// where `std::fs::rename` fails with EXDEV; the bash original used `mv`
// which copies across devices, so do the same here.
fn atomic_write(dest: &std::path::Path, content: &str) -> std::io::Result<()> {
    let dir = dest.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tmp = dir.join(format!(".monitors.{}.tmp", std::process::id()));
    std::fs::write(&tmp, content)?;
    if std::fs::rename(&tmp, dest).is_err() {
        // cross-device fallback (e.g. tmpfs → home): copy then remove
        let r = std::fs::copy(&tmp, dest);
        let _ = std::fs::remove_file(&tmp);
        r?;
    }
    Ok(())
}


fn jq_filter(json: &str, filter: &str) -> String {
    let script = format!("printf '%s' {} | jq -r {}", shell_quote(json), shell_quote(filter));
    let (code, out, _) = run("sh", &["-c", &script]);
    if code == 0 { out } else { String::new() }
}

fn hypr_monitors(all: bool) -> String {
    let mut args = vec!["monitors"];
    if all { args.push("all"); }
    args.push("-j");
    let a: Vec<&str> = args.to_vec();
    let (_, out, _) = run("hyprctl", &a);
    out
}

fn hypr_json(all: bool) -> Value {
    let out = hypr_monitors(all);
    serde_json::from_str(&out).unwrap_or(Value::Array(vec![]))
}

pub fn focused_name() -> String {
    let v = hypr_json(false);
    v.as_array().and_then(|a| a.iter().find(|m| m.get("focused").and_then(|x| x.as_bool()).unwrap_or(false)))
        .and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string()
}

fn external_name() -> String {
    let v = hypr_json(true);
    v.as_array().and_then(|a| a.iter().find(|m| {
        !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false)
            && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("eDP")
            && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("LVDS")
            && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("DSI")
    })).and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string()
}

fn internal_name() -> String {
    let v = hypr_json(true);
    let arr = v.as_array().cloned().unwrap_or_default();
    let name = arr.iter().find(|m| m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("eDP"))
        .and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string();
    if name.is_empty() {
        arr.first().and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string()
    } else { name }
}


fn field_of(name: &str, key: &str) -> String {
    let json = hypr_monitors(true);
    let filter = format!(r#".[] | select(.name == "{}") | .{}"#, name, key);
    jq_filter(&json, &filter).trim().to_string()
}

fn read_state(name: &str) -> Option<MonitorState> {
    let json = hypr_monitors(true);
    let filter = format!(
        r#". as $all | ($all | map({{ key: (.id | tostring), value: .name }}) | from_entries) as $names | $all[] | select(.name == "{}") | [.scale, .transform, .disabled, (if (.mirrorOf // "none") == "none" or (.mirrorOf // "") == "" then "none" else ($names[.mirrorOf] // .mirrorOf) end), "\(.x)x\(.y)"] | @tsv"#,
        name
    );
    let live = jq_filter(&json, &filter).trim().to_string();
    if live.is_empty() { return None; }
    let mut toks: Vec<&str> = live.split('\t').collect();
    if toks.len() < 5 { toks = live.split_whitespace().collect(); }
    if toks.len() < 5 { return None; }
    let mut scale = toks[0].to_string();
    let mut transform = toks[1].to_string();
    let mut disabled = toks[2].to_string();
    let mut mirror = toks[3].to_string();
    let mut position = toks[4].to_string();
    if scale.is_empty() || scale=="0" || scale=="0.0" || scale=="null" { scale="1".to_string(); }
    if transform.is_empty() || transform=="null" { transform="0".to_string(); }
    disabled = if disabled == "true" { "1".to_string() } else { "0".to_string() };
    if mirror.is_empty() || mirror=="null" { mirror="none".to_string(); }
    if position.is_empty() || position=="null" { position="auto".to_string(); }
    if disabled == "1" { position = "auto".to_string(); }

    let mut mode = "preferred".to_string();
    let mut sdrb = "1".to_string();
    let mut sdrr = "1".to_string();

    if let Ok(content) = std::fs::read_to_string(state_file()) {
        for line in content.lines() {
            let t: Vec<&str> = line.split_whitespace().collect();
            if !t.is_empty() && t[0] == name {
                if t.len() >= 2 && !t[1].is_empty() { scale = t[1].to_string(); }
                if t.len() >= 3 && !t[2].is_empty() { transform = t[2].to_string(); }
                if t.len() >= 4 && !t[3].is_empty() {
                    disabled = if t[3] == "true" || t[3] == "1" || t[3] == "yes" { "1".to_string() } else { "0".to_string() };
                }
                if t.len() >= 5 && !t[4].is_empty() { mirror = t[4].to_string(); }
                if t.len() >= 6 && !t[5].is_empty() { position = t[5].to_string(); }
                if t.len() >= 7 && !t[6].is_empty() { mode = t[6].strip_suffix("Hz").unwrap_or(t[6]).to_string(); }
                if t.len() >= 8 && !t[7].is_empty() { sdrb = t[7].to_string(); }
                if t.len() >= 9 && !t[8].is_empty() { sdrr = t[8].to_string(); }
            }
        }
    }

    Some((scale, transform, disabled, mirror, position, mode, sdrb, sdrr))
}

fn apply_change(output: &str, key: &str, value: &str) -> Result<(), String> {
    let (scale, transform, disabled, mirror, position, mode, sdrb, sdrr) =
        read_state(output).ok_or_else(|| format!("no output called \"{}\"", output))?;
    let mut scale = scale; let mut transform = transform; let mut disabled = disabled;
    let mut mirror = mirror; let mut position = position; let mut mode = mode;
    let mut sdrb = sdrb; let mut sdrr = sdrr;
    match key {
        "scale" => scale = value.to_string(),
        "transform" => transform = value.to_string(),
        "disabled" => disabled = value.to_string(),
        "mirror" => mirror = value.to_string(),
        "position" => position = value.to_string(),
        "mode" => mode = value.to_string(),
        "sdrbrightness" => sdrb = value.to_string(),
        "sdrsaturation" => sdrr = value.to_string(),
        _ => return Err(format!("unknown monitor setting \"{}\"", key)),
    }
    let disabled_bool = if disabled == "1" || disabled == "true" { "true" } else { "false" };
    let mirror_val = if mirror == "none" { String::new() } else { mirror.clone() };
    let rule = format!(
        "hl.monitor({{ output = \"{}\", mode = \"{}\", position = \"{}\", scale = {}, transform = {}, disabled = {}, mirror = \"{}\", sdrbrightness = {}, sdrsaturation = {} }})",
        output, mode, position, scale, transform, disabled_bool, mirror_val, sdrb, sdrr
    );
    let (code, _, err) = run("hyprctl", &["eval", &rule]);
    if code != 0 { return Err(format!("hyprctl eval failed: {}", err)); }
    Ok(())
}

fn save_field(output: &str, key: &str, value: &str) -> Result<(), String> {
    let (scale, transform, disabled, mirror, position, mode, sdrb, sdrr) =
        read_state(output).ok_or_else(|| format!("no output called \"{}\"", output))?;
    let mut scale = scale; let mut transform = transform; let mut disabled = disabled;
    let mut mirror = mirror; let mut position = position; let mut mode = mode;
    let mut sdrb = sdrb; let mut sdrr = sdrr;
    match key {
        "scale" => scale = value.to_string(),
        "transform" => transform = value.to_string(),
        "disabled" => disabled = value.to_string(),
        "mirror" => mirror = value.to_string(),
        "position" => position = value.to_string(),
        "mode" => mode = value.to_string(),
        "sdrbrightness" => sdrb = value.to_string(),
        "sdrsaturation" => sdrr = value.to_string(),
        _ => return Err(format!("unknown monitor setting \"{}\"", key)),
    }
    let mirror = if mirror == "none" { "none".to_string() } else { mirror };
    let _ = std::fs::create_dir_all(state_dir());
    let path = state_file();
    let mut lines: Vec<String> = std::fs::read_to_string(&path).unwrap_or_default().lines().map(|s| s.to_string()).collect();
    lines.retain(|l| {
        let t: Vec<&str> = l.split_whitespace().collect();
        t.is_empty() || t[0] != output
    });
    lines.push(format!("{} {} {} {} {} {} {} {} {}", output, scale, transform, disabled, mirror, position, mode, sdrb, sdrr));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

fn read_saved_map() -> std::collections::HashMap<String, (String, String)> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string(state_file()) {
        for line in content.lines() {
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.is_empty() { continue; }
            let name = t[0].to_string();
            let position = t.get(5).filter(|s| !s.is_empty()).unwrap_or(&"auto").to_string();
            let mode = t.get(6).filter(|s| !s.is_empty()).unwrap_or(&"preferred").to_string();
            // normalize a saved disabled/mode that an older or broken port left
            let mode = mode.strip_suffix("Hz").unwrap_or(&mode).to_string();
            map.insert(name, (position, mode));
        }
    }
    map
}

fn list_action() -> String {
    let json = hypr_monitors(true);
    let mut v: Value = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(_) => return json.trim().to_string(),
    };
    let arr = match v.as_array_mut() {
        Some(a) => a,
        None => return json.trim().to_string(),
    };

    // id -> name
    let mut id_to_name = std::collections::HashMap::new();
    for m in arr.iter() {
        if let (Some(id), Some(name)) = (m.get("id"), m.get("name")) {
            if let (Some(i), Some(n)) = (id.as_i64(), name.as_str()) {
                id_to_name.insert(i, n.to_string());
            }
        }
    }

    let saved = read_saved_map();

    for m in arr.iter_mut() {
        let name = m.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();

        // resolve mirrorOf (id -> name)
        let mirror_raw = m.get("mirrorOf").cloned().unwrap_or(Value::Null);
        let mut mirror = match mirror_raw {
            Value::String(s) => s,
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => if b { "true".to_string() } else { "none".to_string() },
            _ => "none".to_string(),
        };
        if mirror.is_empty() { mirror = "none".to_string(); }
        if mirror != "none" {
            if let Ok(id) = mirror.parse::<i64>() {
                if let Some(n) = id_to_name.get(&id) {
                    mirror = n.clone();
                }
            }
        }

        let (pos, mode) = saved.get(&name).cloned().unwrap_or_else(|| ("auto".to_string(), "preferred".to_string()));

        // availableModes: strip trailing Hz, unique
        let mut seen = std::collections::HashSet::new();
        let mut modes_out: Vec<Value> = Vec::new();
        if let Some(arr_modes) = m.get("availableModes").and_then(|x| x.as_array()) {
            for x in arr_modes {
                if let Some(s) = x.as_str() {
                    let s = s.strip_suffix("Hz").unwrap_or(s);
                    if seen.insert(s.to_string()) {
                        modes_out.push(Value::String(s.to_string()));
                    }
                }
            }
        }

        if let Some(obj) = m.as_object_mut() {
            obj.insert("mirrorOf".to_string(), Value::String(mirror));
            obj.insert("savedPosition".to_string(), Value::String(pos));
            obj.insert("savedMode".to_string(), Value::String(mode));
            obj.insert("availableModes".to_string(), Value::Array(modes_out));
        }
    }

    serde_json::to_string(&v).unwrap_or_else(|_| json.trim().to_string())
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        return "ERR monitors: missing subcommand".to_string();
    }
    match args[0].as_str() {
        "status" => {
            let v = hypr_json(true);
            let arr = v.as_array().cloned().unwrap_or_default();
            // map id -> name
            let mut id_to_name = std::collections::HashMap::new();
            for m in &arr {
                if let (Some(id), Some(name)) = (m.get("id"), m.get("name")) {
                    if let (Some(i), Some(n)) = (id.as_i64(), name.as_str()) {
                        id_to_name.insert(i, n.to_string());
                    }
                }
            }
            let mut rows: Vec<Vec<String>> = Vec::new();
            for m in &arr {
                let name = m.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if name.is_empty() { continue; }
                let w = m.get("width").and_then(|x| x.as_i64()).unwrap_or(0);
                let h = m.get("height").and_then(|x| x.as_i64()).unwrap_or(0);
                let res = format!("{}x{}", w, h);
                let scale = m.get("scale").map(|x| x.to_string()).unwrap_or_else(|| "1".to_string());
                let scale = if scale.is_empty() || scale == "null" { "1".to_string() } else { scale };
                let disabled = m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false);
                let mirror = m.get("mirrorOf").and_then(|x| x.as_str()).unwrap_or("none");
                let mode = if disabled {
                    "off".to_string()
                } else if mirror == "none" || mirror.is_empty() {
                    "extended".to_string()
                } else {
                    let mirror_name = mirror.parse::<i64>().ok().and_then(|id| id_to_name.get(&id).cloned()).unwrap_or_else(|| mirror.to_string());
                    format!("mirror of {}", mirror_name)
                };
                let focus = if m.get("focused").and_then(|x| x.as_bool()).unwrap_or(false) { "yes" } else { "" }.to_string();
                rows.push(vec![name, res, scale, mode, focus]);
            }
            if rows.is_empty() {
                "No monitors.".to_string()
            } else {
                crate::table::render(&["Monitor", "Resolution", "Scale", "Mode", "Focus"], &rows)
            }
        }
        "list" => list_action(),
        "set" => {
            if args.len() < 4 {
                return "ERR usage: monitors set OUTPUT KEY VALUE".to_string();
            }
            set_action(&args[1], &args[2], &args[3])
        }
        "scale" => {
            let focused = focused_name();
            if focused.is_empty() { return "ERR no focused monitor".to_string(); }
            let action = if args.len() >= 2 { args[1].as_str() } else { "up" };
            match action {
                "set" => {
                    if args.len() < 3 { return "ERR usage: monitors scale set N".to_string(); }
                    let v = &args[2];
                    let n: f64 = match v.parse() { Ok(x) => x, Err(_) => return "ERR bad scale".to_string() };
                    if n <= 0.0 { return "ERR scale must be > 0".to_string(); }
                    if let Err(e) = apply_change(&focused, "scale", v) { return format!("ERR {}", e); }
                    if let Err(e) = save_field(&focused, "scale", v) { return format!("ERR {}", e); }
                    format!("{} scale set to {}", focused, v)
                }
                "reset" => {
                    if let Err(e) = apply_change(&focused, "scale", "1") { return format!("ERR {}", e); }
                    if let Err(e) = save_field(&focused, "scale", "1") { return format!("ERR {}", e); }
                    format!("{} scale reset", focused)
                }
                "up" | "down" => {
                    let ladder = ["1", "1.25", "1.5", "1.6", "2", "3", "4"];
                    let cur = field_of(&focused, "scale");
                    let cur = if cur.is_empty() { "1".to_string() } else { cur };
                    let mut idx = ladder.iter().position(|&s| s == cur).unwrap_or(0);
                    if action == "up" { if idx + 1 < ladder.len() { idx += 1; } }
                    else { idx = idx.saturating_sub(1); }
                    let newv = ladder[idx];
                    if let Err(e) = apply_change(&focused, "scale", newv) { return format!("ERR {}", e); }
                    if let Err(e) = save_field(&focused, "scale", newv) { return format!("ERR {}", e); }
                    format!("{} scale {}", focused, newv)
                }
                _ => format!("ERR unknown scale action: {}", action),
            }
        }
        "brightness" => {
            let focused = focused_name();
            if focused.is_empty() { return "ERR no focused monitor".to_string(); }
            let action = if args.len() >= 2 { args[1].as_str() } else { "up" };
            let step: i64 = if args.len() >= 3 { args[2].parse().unwrap_or(5) } else { 5 };
            let is_internal = focused.starts_with("eDP") || focused.starts_with("LVDS") || focused.starts_with("DSI");
            match action {
                "up" => {
                    if is_internal {
                        let (code, out, err) = run("brightnessctl", &["set", &format!("{}%+", step)]);
                        if code != 0 { return format!("ERR brightnessctl: {}", err); }
                        format!("{} brightness {}", focused, out.trim())
                    } else {
                        let (code, _, err) = run("ddcutil", &["setvcp", "10", &format!("+ {}", step)]);
                        if code != 0 { return format!("ERR ddcutil: {}", err); }
                        "OK".to_string()
                    }
                }
                "down" => {
                    if is_internal {
                        let (code, out, err) = run("brightnessctl", &["set", &format!("{}%-", step)]);
                        if code != 0 { return format!("ERR brightnessctl: {}", err); }
                        format!("{} brightness {}", focused, out.trim())
                    } else {
                        let (code, _, err) = run("ddcutil", &["setvcp", "10", &format!("- {}", step)]);
                        if code != 0 { return format!("ERR ddcutil: {}", err); }
                        "OK".to_string()
                    }
                }
                "max" => {
                    if is_internal { let _ = run("brightnessctl", &["set", "100%"]); }
                    else { let _ = run("ddcutil", &["setvcp", "10", "100"]); }
                    "OK".to_string()
                }
                "min" => {
                    if is_internal { let _ = run("brightnessctl", &["set", "1%"]); }
                    else { let _ = run("ddcutil", &["setvcp", "10", "1"]); }
                    "OK".to_string()
                }
                "set" => {
                    if args.len() < 3 { return "ERR usage: monitors brightness set N".to_string(); }
                    let val = &args[2];
                    if is_internal {
                        let (code, _, err) = run("brightnessctl", &["set", &format!("{}%", val)]);
                        if code != 0 { return format!("ERR brightnessctl: {}", err); }
                    } else {
                        let (code, _, err) = run("ddcutil", &["setvcp", "10", val]);
                        if code != 0 { return format!("ERR ddcutil: {}", err); }
                    }
                    "OK".to_string()
                }
                _ => format!("ERR unknown brightness action: {}", action),
            }
        }
        "identify" => {
            let json = hypr_monitors(true);
            let names = jq_filter(&json, r#".[].name"#);
            for name in names.lines() {
                let name = name.trim();
                if name.is_empty() { continue; }
                let _ = run("notify-send", &["-t", "2000", "Monitor", name]);
            }
            "OK".to_string()
        }
        "wake" => {
            let _ = run("hyprctl", &["dispatch", "dpms", "on"]);
            "OK".to_string()
        }
        "recover" => recover_action(),
        "internal" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            internal_action(action)
        }
        "mirror" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            mirror_action(action)
        }
        "lid" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            lid_action(action)
        }
        "try" => {
            if args.len() < 4 { return "ERR try needs an output, a setting and a value".to_string(); }
            try_action(&args[1..])
        }
        "keep" => keep_action(),
        "revert" => revert_action(),
        "workspaces" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "sync" };
            workspaces_action(action)
        }
        "workspace" => {
            let slot = if args.len() >= 2 { args[1].as_str() } else { "" };
            workspace_action(slot)
        }
        "workspace-step" => {
            let dir = if args.len() >= 2 { args[1].as_str() } else { "next" };
            workspace_step_action(dir)
        }
        "move-window" => {
            let slot = if args.len() >= 2 { args[1].as_str() } else { "" };
            move_window_action(slot)
        }
        "identities" => identities_action(),
        "sync-identities" => sync_identities_action(),
        _ => format!("ERR unknown monitors subcommand: {}", args[0]),
    }
}
// ---------- helpers ----------

fn parse_transform(v: &str) -> Option<usize> {
    if let Some(i) = transform_index(v) { return Some(i); }
    if let Ok(i) = v.parse::<usize>() {
        if i < 8 { return Some(i); }
    }
    None
}

fn valid_int(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn valid_mode(v: &str) -> bool {
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

fn valid_position(v: &str) -> bool {
    if matches!(v, "auto" | "auto-left" | "auto-right" | "auto-up" | "auto-down") { return true; }
    let mut xb = v.splitn(2, 'x');
    let x = xb.next().unwrap_or("");
    let y = xb.next().unwrap_or("");
    if xb.count() > 0 { return false; }
    if !v.contains('x') { return false; }
    valid_int(x) && valid_int(y)
}

fn set_action(output: &str, key: &str, value: &str) -> String {
    // refuse unknown output
    if field_of(output, "name").is_empty() {
        return format!("ERR no output called \"{}\"", output);
    }

    match key {
        "scale" => {
            let ladder = ["1", "1.25", "1.5", "1.6", "2", "3", "4"];
            if !ladder.contains(&value) {
                return "ERR scale must be one of: 1 1.25 1.5 1.6 2 3 4".to_string();
            }
            if let Err(e) = apply_change(output, "scale", value) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "scale", value) { return format!("ERR {}", e); }
            format!("{} scale {}", output, value)
        }
        "transform" => {
            let idx = match parse_transform(value) {
                Some(i) => i,
                None => return "ERR transform must be one of: normal 90 180 270 flipped flipped-90 flipped-180 flipped-270".to_string(),
            };
            let idx_s = idx.to_string();
            if let Err(e) = apply_change(output, "transform", &idx_s) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "transform", &idx_s) { return format!("ERR {}", e); }
            format!("{} transform {}", output, value)
        }
        "disabled" => {
            let (index, disabled) = match value {
                "yes" | "true" | "on" => (1, "true"),
                "no" | "false" | "off" => (0, "false"),
                _ => return "ERR disabled takes yes or no".to_string(),
            };
            if index == 1 && field_of(output, "disabled") != "true" && active_count_i() <= 1 {
                return "ERR refusing to disable the only active output".to_string();
            }
            if let Err(e) = apply_change(output, "disabled", disabled) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "disabled", &index.to_string()) { return format!("ERR {}", e); }
            if index == 0 {
                let _ = run("sh", &["-c", "monitors workspaces sync >/dev/null 2>&1 || true"]);
            }
            format!("{} disabled {}", output, value)
        }
        "position" => {
            if !valid_position(value) {
                return "ERR position must be auto, auto-left/right/up/down, or XxY".to_string();
            }
            if let Err(e) = apply_change(output, "position", value) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "position", value) { return format!("ERR {}", e); }
            format!("{} position {}", output, value)
        }
        "mode" => {
            let v = value.strip_suffix("Hz").unwrap_or(value).trim();
            if !valid_mode(v) {
                return "ERR mode must be preferred, highres, highrr, or WxH[@R]".to_string();
            }
            if let Err(e) = apply_change(output, "mode", v) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "mode", v) { return format!("ERR {}", e); }
            format!("{} mode {}", output, v)
        }
        "sdrbrightness" => {
            if !value.chars().all(|c| c.is_ascii_digit() || c == '.') || value.is_empty() {
                return "ERR sdrbrightness must be a number".to_string();
            }
            if let Err(e) = apply_change(output, "sdrbrightness", value) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "sdrbrightness", value) { return format!("ERR {}", e); }
            format!("{} sdrbrightness {}", output, value)
        }
        "sdrsaturation" => {
            if !value.chars().all(|c| c.is_ascii_digit() || c == '.') || value.is_empty() {
                return "ERR sdrsaturation must be a number".to_string();
            }
            if let Err(e) = apply_change(output, "sdrsaturation", value) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "sdrsaturation", value) { return format!("ERR {}", e); }
            format!("{} sdrsaturation {}", output, value)
        }
        "mirror" => {
            let mut v = value.to_string();
            if v == "off" || v.is_empty() { v = "none".to_string(); }
            if v != "none" {
                if v == output { return "ERR an output cannot mirror itself".to_string(); }
                if field_of(&v, "name").is_empty() { return format!("ERR no output called \"{}\" to mirror", v); }
                if field_of(&v, "disabled") == "true" { return format!("ERR {} is off; turn it on before mirroring", v); }
                // target must not already be mirroring
                let st = read_state(&v);
                if let Some(s) = st {
                    if s.3 != "none" { return format!("ERR {} is mirroring another screen; extend it first", v); }
                }
            }
            if v != "none" {
                if field_of(output, "disabled") == "true" {
                    return format!("ERR {} is off; turn it on before mirroring", output);
                }
            } else {
                if field_of(output, "disabled") == "true" {
                    return format!("ERR {} is off; turn it on before mirroring", output);
                }
            }
            if let Err(e) = apply_change(output, "mirror", &v) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, "mirror", &v) { return format!("ERR {}", e); }
            if v == "none" {
                let _ = run("sh", &["-c", "monitors workspaces sync >/dev/null 2>&1 || true"]);
            }
            if v == "none" { format!("{} extended again", output) } else { format!("{} now mirrors {}", output, v) }
        }
        _ => "ERR set takes scale, transform, disabled, position, mode, mirror, sdrbrightness or sdrsaturation".to_string(),
    }
}

fn try_action(args: &[String]) -> String {
    if args.len() < 3 { return "ERR try needs an output, a setting and a value".to_string(); }
    let output = &args[0]; let key = &args[1]; let value = &args[2];
    if output.is_empty() || !output.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return format!("ERR invalid output name: {}", output);
    }
    let pending = state_dir().join("display-pending");
    if pending.exists() { return "ERR a display change is already pending; run keep or revert first".to_string(); }
    if field_of(output, "name").is_empty() { return format!("ERR no output called \"{}\"", output); }
    if field_of(output, "disabled") == "true" { return format!("ERR {} is off; turn it on first", output); }

    let st = match read_state(output) { Some(s) => s, None => return format!("ERR no output called \"{}\"", output) };

    let mut value = value.clone();
    let old = match key.as_str() {
        "scale" => {
            let ladder = ["1", "1.25", "1.5", "1.6", "2", "3", "4"];
            if !ladder.contains(&value.as_str()) { return "ERR scale must be one of: 1 1.25 1.5 1.6 2 3 4".to_string(); }
            st.0.clone()
        }
        "transform" => {
            let idx = match parse_transform(&value) {
                Some(i) => i,
                None => return "ERR transform must be one of: normal 90 180 270 flipped flipped-90 flipped-180 flipped-270".to_string(),
            };
            value = idx.to_string();
            st.1.clone()
        }
        "mode" => {
            let v = value.strip_suffix("Hz").unwrap_or(&value).trim().to_string();
            if !valid_mode(&v) { return "ERR mode must be preferred, highres, highrr, or WxH[@R]".to_string(); }
            value = v;
            st.5.clone()
        }
        "position" => {
            if !valid_position(&value) { return "ERR position must be auto, auto-left/right/up/down, or XxY".to_string(); }
            st.4.clone()
        }
        _ => return "ERR try takes scale, transform, mode or position".to_string(),
    };

    let _ = std::fs::create_dir_all(state_dir());
    let content = format!("{}\t{}\t{}\t{}\n", output, key, old, value);
    if atomic_write(&pending, &content).is_err() { return "ERR could not write pending".to_string(); }

    // arm timer
    let _ = run("systemctl", &["--user", "stop", "anixie-display-revert.timer"]);
    let mut args: Vec<String> = vec![
        "--user".to_string(), "--quiet".to_string(), "--collect".to_string(),
        "--unit=anixie-display-revert".to_string(),
        "--on-active=15".to_string(),
    ];
    if let Ok(v) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") { args.push(format!("--setenv=HYPRLAND_INSTANCE_SIGNATURE={}", v)); }
    if let Ok(v) = std::env::var("XDG_RUNTIME_DIR") { args.push(format!("--setenv=XDG_RUNTIME_DIR={}", v)); }
    if let Ok(v) = std::env::var("XDG_STATE_HOME") { args.push(format!("--setenv=XDG_STATE_HOME={}", v)); }
    if let Ok(v) = std::env::var("HOME") { args.push(format!("--setenv=HOME={}", v)); }
    args.push("--".to_string());
    args.push("alexie".to_string());
    args.push("monitors".to_string());
    args.push("revert".to_string());
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, _, err) = run("systemd-run", &a);
    if code != 0 {
        let _ = std::fs::remove_file(&pending);
        return format!("ERR could not schedule the automatic revert: {}", err.trim());
    }

    let apply_val = value.clone();
    if let Err(e) = apply_change(output, key, &apply_val) { return format!("ERR {}", e); }

    format!("{} {} {} (reverting in 15s unless kept)", output, key, value)
}

fn transform_index(name: &str) -> Option<usize> {
    let names = ["normal","90","180","270","flipped","flipped-90","flipped-180","flipped-270"];
    names.iter().position(|&n| n == name)
}

fn lid_closed() -> bool {
    // read /proc/acpi/button/lid/*/state
    if let Ok(entries) = std::fs::read_dir("/proc/acpi/button/lid") {
        for e in entries.flatten() {
            let p = e.path().join("state");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if s.contains("closed") { return true; }
            }
        }
    }
    false
}

fn clear_disabled(output: &str) {
    let path = state_file();
    if !path.exists() { return; }
    if let Ok(content) = std::fs::read_to_string(&path) {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut changed = false;
        for line in lines.iter_mut() {
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.len() >= 4 && t[0] == output && t[3] == "1" {
                // set 4th field (index 3) to 0
                let mut f: Vec<String> = t.iter().map(|s| s.to_string()).collect();
                while f.len() < 9 { f.push(String::new()); }
                f[3] = "0".to_string();
                *line = f.join(" ");
                changed = true;
            }
        }
        if changed {
            let _ = std::fs::write(&path, lines.join("\n") + "\n");
        }
    }
}

fn active_count_i() -> i64 {
    let v = hypr_json(true);
    v.as_array().map(|a| a.iter().filter(|m| !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false)).count() as i64).unwrap_or(0)
}

fn internal_action(action: &str) -> String {
    let name = internal_name();
    if name.is_empty() { return "ERR no monitor found".to_string(); }
    let disabled = field_of(&name, "disabled");
    let action = if action == "toggle" {
        if disabled == "true" { "on" } else { "off" }
    } else { action };
    match action {
        "off" => {
            if disabled == "true" { return "OK".to_string(); }
            if active_count_i() <= 1 { return "ERR refusing to disable the only active output".to_string(); }
            if let Err(e) = apply_change(&name, "disabled", "true") { return format!("ERR {}", e); }
            if let Err(e) = save_field(&name, "disabled", "1") { return format!("ERR {}", e); }
            format!("internal panel {}: off", name)
        }
        "on" => {
            if disabled != "true" { return "OK".to_string(); }
            if let Err(e) = apply_change(&name, "disabled", "false") { return format!("ERR {}", e); }
            if let Err(e) = save_field(&name, "disabled", "0") { return format!("ERR {}", e); }
            let _ = run("sh", &["-c", "monitors workspaces sync >/dev/null 2>&1 || true"]);
            format!("internal panel {}: on", name)
        }
        _ => "ERR internal takes on, off or toggle".to_string(),
    }
}

fn mirror_action(action: &str) -> String {
    let internal = internal_name();
    let external = external_name();
    if external.is_empty() { return "ERR no external monitor connected to mirror onto".to_string(); }
    if field_of(&internal, "disabled") == "true" {
        return "ERR internal panel is off; turn it on before mirroring".to_string();
    }
    let mirrored = field_of(&external, "mirrorOf");
    let mirrored = if mirrored.is_empty() || mirrored == "null" { "none".to_string() } else { mirrored };
    let action = if action == "toggle" {
        if mirrored == "none" { "on" } else { "off" }
    } else { action };
    match action {
        "on" => {
            if let Err(e) = apply_change(&external, "mirror", &internal) { return format!("ERR {}", e); }
            if let Err(e) = save_field(&external, "mirror", &internal) { return format!("ERR {}", e); }
            format!("{} now mirrors {}", external, internal)
        }
        "off" => {
            if let Err(e) = apply_change(&external, "mirror", "none") { return format!("ERR {}", e); }
            if let Err(e) = save_field(&external, "mirror", "none") { return format!("ERR {}", e); }
            let _ = run("sh", &["-c", "monitors workspaces sync >/dev/null 2>&1 || true"]);
            format!("{} extended again", external)
        }
        _ => "ERR mirror takes on, off or toggle".to_string(),
    }
}

fn lid_action(action: &str) -> String {
    let internal = internal_name();
    if internal.is_empty() { return "OK".to_string(); }
    let external = external_name();
    match action {
        "close" | "off" => {
            if external.is_empty() {
                return "no external monitor; leaving the internal panel on".to_string();
            }
            if field_of(&internal, "disabled") == "true" { return "OK".to_string(); }
            if let Err(e) = apply_change(&internal, "disabled", "true") { return format!("ERR {}", e); }
            if let Err(e) = save_field(&internal, "disabled", "1") { return format!("ERR {}", e); }
            let _ = std::fs::create_dir_all(state_dir());
            let _ = std::fs::write(lid_file(), format!("{}\n", internal));
            format!("lid closed: internal panel {} off", internal)
        }
        "open" | "on" => {
            let saved = match std::fs::read_to_string(lid_file()) { Ok(s) => s.trim().to_string(), Err(_) => String::new() };
            if saved.is_empty() { return "OK".to_string(); }
            if field_of(&saved, "name").is_empty() { let _ = std::fs::remove_file(lid_file()); return "OK".to_string(); }
            if let Err(e) = apply_change(&saved, "disabled", "false") { return format!("ERR {}", e); }
            if let Err(e) = save_field(&saved, "disabled", "0") { return format!("ERR {}", e); }
            let _ = std::fs::remove_file(lid_file());
            let _ = run("sh", &["-c", "monitors workspaces sync >/dev/null 2>&1 || true"]);
            format!("lid opened: internal panel {} on", saved)
        }
        "toggle" => {
            if lid_file().exists() { lid_action("open") } else { lid_action("close") }
        }
        "reconcile" => {
            if lid_closed() && !external.is_empty() { lid_action("close") } else { lid_action("open") }
        }
        _ => "ERR lid takes close, open, toggle or reconcile".to_string(),
    }
}

fn recover_action() -> String {
    // clear stale lid state if lid is open
    if lid_file().exists() && !lid_closed() {
        if let Ok(saved) = std::fs::read_to_string(lid_file()) {
            let saved = saved.trim().to_string();
            if !saved.is_empty() && !field_of(&saved, "name").is_empty() {
                clear_disabled(&saved);
            }
        }
        let _ = std::fs::remove_file(lid_file());
        return "lid is open; cleared the saved clamshell state".to_string();
    }
    "OK".to_string()
}
fn keep_action() -> String {
    let pending = state_dir().join("display-pending");
    if !pending.exists() { return "no display change to keep".to_string(); }
    let content = match std::fs::read_to_string(&pending) { Ok(s) => s, Err(_) => return "ERR could not read pending".to_string() };
    let mut parts = content.trim().split('\t');
    let output = parts.next().unwrap_or("").to_string();
    let key = parts.next().unwrap_or("").to_string();
    let _old = parts.next().unwrap_or("").to_string();
    let mut new = parts.next().unwrap_or("").to_string();
    if key == "mode" { new = new.strip_suffix("Hz").unwrap_or(&new).to_string(); }
    if output.is_empty() || key.is_empty() { let _ = std::fs::remove_file(&pending); return "ERR could not read pending".to_string(); }

    let _ = run("systemctl", &["--user", "stop", "anixie-display-revert.timer"]);
    if let Err(e) = save_field(&output, &key, &new) { return format!("ERR {}", e); }
    let _ = std::fs::remove_file(&pending);
    format!("{} {} {} kept", output, key, new)
}

fn revert_action() -> String {
    let pending = state_dir().join("display-pending");
    if !pending.exists() { return "OK".to_string(); }
    let content = match std::fs::read_to_string(&pending) { Ok(s) => s, Err(_) => return "OK".to_string() };
    let mut parts = content.trim().split('\t');
    let output = parts.next().unwrap_or("").to_string();
    let key = parts.next().unwrap_or("").to_string();
    let mut old = parts.next().unwrap_or("").to_string();
    if key == "mode" { old = old.strip_suffix("Hz").unwrap_or(&old).to_string(); }
    let _new = parts.next().unwrap_or("").to_string();

    if field_of(&output, "name").is_empty() {
        let _ = std::fs::remove_file(&pending);
        return "OK".to_string();
    }

    let _ = run("systemctl", &["--user", "stop", "anixie-display-revert.timer"]);
    if let Err(e) = apply_change(&output, &key, &old) { let _ = std::fs::remove_file(&pending); return format!("ERR {}", e); }
    let _ = std::fs::remove_file(&pending);
    format!("{} {} reverted to {}", output, key, old)
}

fn workspace_file() -> std::path::PathBuf { state_dir().join("workspaces.json") }

fn workspace_base(name: &str) -> String {
    if !workspace_file().exists() {
        return "1".to_string();
    }
    let _filter = format!(r#".["{}"] // empty"#, name);
    // workspace_file is a JSON object; use jq on file
    let (code, out, _) = run("jq", &["-r", &format!(r#".["{}"] // empty"#, name), workspace_file().to_str().unwrap()]);
    if code == 0 && !out.trim().is_empty() { out.trim().to_string() } else { "1".to_string() }
}


fn workspaces_sync_action() -> String {
    let _ = std::fs::create_dir_all(state_dir());
    // read map
    let mut map = String::from("{}");
    if workspace_file().exists() {
        if let Ok(c) = std::fs::read_to_string(workspace_file()) {
            let c = c.trim();
            if !c.is_empty() {
                let (code, _, _) = run("jq", &["-e", "type == \"object\"", "-n", &format!("--argjson m {} '$m'", c)]);
                // simpler: validate with jq on the file content via stdin
                let _ = code;
                map = c.to_string();
            }
        }
    }
    // validate
    {
        let script = format!("printf '%s' {} | jq -e 'type == \"object\"' >/dev/null 2>&1", shell_quote(&map));
        let (code, _, _) = run("sh", &["-c", &script]);
        if code != 0 { map = "{}".to_string(); }
    }


    // names of active monitors
    let json = hypr_monitors(false);
    let names_str = jq_filter(&json, r#".[].name"#);
    let names: Vec<&str> = names_str.lines().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    // all monitors json for identity
    let all_json = hypr_monitors(true);

    for name in &names {
        // identity = serial or name
        let filter = format!(r#".[] | select(.name == "{}") | (.serial // .name)"#, name);
        let mut identity = jq_filter(&all_json, &filter).trim().to_string();
        if identity.is_empty() { identity = name.to_string(); }

        // migrate name-keyed entry to identity key
        let has_name = {
            let script = format!("printf '%s' {} | jq -r --arg n {} 'has($n)'", shell_quote(&map), shell_quote(name));
            let (code, out, _) = run("sh", &["-c", &script]);
            code == 0 && out.trim() == "true"
        };
        let has_ident = {
            let script = format!("printf '%s' {} | jq -r --arg i {} 'has($i)'", shell_quote(&map), shell_quote(&identity));
            let (code, out, _) = run("sh", &["-c", &script]);
            code == 0 && out.trim() == "true"
        };
        if has_name && !has_ident {
            let base = {
                let script = format!("printf '%s' {} | jq -r --arg n {} '.[$n]'", shell_quote(&map), shell_quote(name));
                let (_, out, _) = run("sh", &["-c", &script]);
                out.trim().to_string()
            };
            let script = format!(
                "printf '%s' {} | jq --arg n {} --arg i {} --argjson b {} 'del(.[$n]) + {{($i): $b}}'",
                shell_quote(&map), shell_quote(name), shell_quote(&identity), base
            );
            let (_, out, _) = run("sh", &["-c", &script]);
            if !out.trim().is_empty() { map = out.trim().to_string(); }
        }
        let has_ident2 = {
            let script = format!("printf '%s' {} | jq -r --arg i {} 'has($i)'", shell_quote(&map), shell_quote(&identity));
            let (code, out, _) = run("sh", &["-c", &script]);
            code == 0 && out.trim() == "true"
        };
        if !has_ident2 {
            // find candidate
            let mut candidate = 1i64;
            loop {
                let script = format!("printf '%s' {} | jq -r --argjson b {} 'any(.[]; . == $b)'", shell_quote(&map), candidate);
                let (_, out, _) = run("sh", &["-c", &script]);
                if out.trim() != "true" { break; }
                candidate += 10;
            }
            let script = format!("printf '%s' {} | jq --arg i {} --argjson b {} '. + {{($i): $b}}'", shell_quote(&map), shell_quote(&identity), candidate);
            let (_, out, _) = run("sh", &["-c", &script]);
            if !out.trim().is_empty() { map = out.trim().to_string(); }
        }
    }

    // write map to workspace_file
    let _ = std::fs::create_dir_all(state_dir());
    let (_code, _out, _) = run("jq", &["-S", ".", "-n", &format!("--argjson m {} '$m'", map)]);
    // simpler: pipe map through jq -S .
    let script = format!("printf '%s' {} | jq -S .", shell_quote(&map));
    let (_, sorted, _) = run("sh", &["-c", &script]);
    let to_write = if sorted.trim().is_empty() { map.clone() } else { sorted.trim().to_string() };
    let _ = atomic_write(&workspace_file(), &format!("{}\n", to_write));

    // build lua and hyprctl eval
    let mut lua = String::new();
    for name in &names {
        // find base by identity? Use name as key first (map keyed by identity). Need identity for name.
        let identity = {
            let filter = format!(r#".[] | select(.name == "{}") | (.serial // .name)"#, name);
            let id = jq_filter(&all_json, &filter).trim().to_string();
            if id.is_empty() { name.to_string() } else { id }
        };
        let base_str = {
            let script = format!("printf '%s' {} | jq -r --arg i {} '.[$i]'", shell_quote(&map), shell_quote(&identity));
            let (_, out, _) = run("sh", &["-c", &script]);
            out.trim().to_string()
        };
        let base: i64 = base_str.parse().unwrap_or(1);
        for i in 1..=10 {
            if i == 1 {
                lua.push_str(&format!("hl.workspace_rule({{ workspace = \"{}\", monitor = \"{}\", default = true }}); ", base, name));
            } else {
                lua.push_str(&format!("hl.workspace_rule({{ workspace = \"{}\", monitor = \"{}\" }}); ", base + i as i64 - 1, name));
            }
        }
    }
    if !lua.is_empty() {
        let _ = run("hyprctl", &["eval", &lua]);
    }
    map
}

fn workspaces_action(action: &str) -> String {
    match action {
        "sync" => { workspaces_sync_action(); "OK".to_string() }
        "show" => {
            if workspace_file().exists() {
                let (code, out, _) = run("jq", &["-S", ".", workspace_file().to_str().unwrap()]);
                if code == 0 { out.trim().to_string() } else { "{}".to_string() }
            } else { "{}".to_string() }
        }
        "reset" => {
            let _ = std::fs::remove_file(workspace_file());
            workspaces_sync_action();
            "OK".to_string()
        }
        _ => "ERR workspaces takes sync, show or reset".to_string(),
    }
}

fn workspace_action(slot: &str) -> String {
    if slot.is_empty() { return "ERR workspace needs a number".to_string(); }
    if !slot.chars().all(|c| c.is_ascii_digit()) { return "ERR workspace needs a number".to_string(); }
    let n: i64 = match slot.parse() { Ok(v) => v, Err(_) => return "ERR workspace needs a number".to_string() };
    if !(1..=10).contains(&n) { return "ERR workspace must be 1-10".to_string(); }
    let focused = focused_name();
    if focused.is_empty() { return "ERR no focused monitor".to_string(); }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    let id = base + n - 1;
    let lua = format!("hl.dispatch(hl.dsp.focus({{ workspace = \"{}\" }}))", id);
    let _ = run("hyprctl", &["eval", &lua]);
    format!("{} workspace {} (global {})", focused, n, id)
}

fn workspace_step_action(direction: &str) -> String {
    if direction != "next" && direction != "prev" { return "ERR workspace-step takes next or prev".to_string(); }
    let focused = focused_name();
    if focused.is_empty() { return "ERR no focused monitor".to_string(); }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    // current focused workspace id
    let json = hypr_monitors(false);
    let cur_str = jq_filter(&json, r#".[] | select(.focused == true) | .activeWorkspace.id"#);
    let current: i64 = match cur_str.trim().parse() { Ok(v) => v, Err(_) => return "OK".to_string() };
    let target = if direction == "next" { current + 1 } else { current - 1 };
    let target = target.clamp(base, base + 9);
    if target == current { return "OK".to_string(); }
    let lua = format!("hl.dispatch(hl.dsp.focus({{ workspace = \"{}\" }}))", target);
    let _ = run("hyprctl", &["eval", &lua]);
    "OK".to_string()
}

fn move_window_action(slot: &str) -> String {
    if slot.is_empty() { return "ERR move-window needs a number".to_string(); }
    if !slot.chars().all(|c| c.is_ascii_digit()) { return "ERR move-window needs a number".to_string(); }
    let n: i64 = match slot.parse() { Ok(v) => v, Err(_) => return "ERR move-window needs a number".to_string() };
    if !(1..=10).contains(&n) { return "ERR move-window must be 1-10".to_string(); }
    let focused = focused_name();
    if focused.is_empty() { return "ERR no focused monitor".to_string(); }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    let id = base + n - 1;
    let lua = format!("hl.dispatch(hl.dsp.window.move({{ workspace = \"{}\" }}))", id);
    let _ = run("hyprctl", &["eval", &lua]);
    format!("window to {} workspace {} (global {})", focused, n, id)
}

fn identities_action() -> String {
    let json = hypr_monitors(true);
    let filter = r#"map({ (.name): { make, model, serial, description } }) | add"#;
    jq_filter(&json, filter).trim().to_string()
}

fn sync_identities_action() -> String {
    let json = hypr_monitors(true);
    let filter = r#"map({ (.name): { make, model, serial, description } }) | add"#;
    let out = jq_filter(&json, filter);
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(state_dir().join("monitor_identities.json"), format!("{}\n", out.trim()));
    "identities saved".to_string()
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
}

#[cfg(test)]
mod more_tests {
    use super::*;

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

    #[test]
    fn read_state_defaults_for_missing_file() {
        // With no state file, read_state should still return live values (or None if no hyprland).
        // In test env (no hyprland), returns None. Just ensure no panic.
        let _ = read_state("NONEXISTENT");
    }

    #[test]
    fn shell_quote_basic() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
