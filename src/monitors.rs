use std::env;
use std::process::Command;

fn state_dir() -> std::path::PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    std::path::PathBuf::from(state).join("anixie")
}

fn state_file() -> std::path::PathBuf { state_dir().join("display.conf") }
fn lid_file() -> std::path::PathBuf { state_dir().join("lid") }

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            (code, out, err)
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
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
    let a: Vec<&str> = args.iter().map(|s| *s).collect();
    let (_, out, _) = run("hyprctl", &a);
    out
}

fn focused_name() -> String {
    let json = hypr_monitors(false);
    jq_filter(&json, r#"[.[] | select(.focused == true)][0].name // empty"#).trim().to_string()
}

fn external_name() -> String {
    let json = hypr_monitors(true);
    jq_filter(&json, r#"[.[] | select(.disabled == false) | select(.name | test("^eDP|^LVDS|^DSI") | not)][0].name // empty"#).trim().to_string()
}

fn internal_name() -> String {
    let json = hypr_monitors(true);
    let name = jq_filter(&json, r#"[.[] | select(.name | test("^eDP"))][0].name // empty"#).trim().to_string();
    if name.is_empty() {
        jq_filter(&json, r#".[0].name // empty"#).trim().to_string()
    } else { name }
}


fn field_of(name: &str, key: &str) -> String {
    let json = hypr_monitors(true);
    let filter = format!(r#".[] | select(.name == "{}") | .{}"#, name, key);
    jq_filter(&json, &filter).trim().to_string()
}

fn read_state(name: &str) -> Option<(String,String,String,String,String,String,String,String)> {
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
                if t.len() >= 4 && !t[3].is_empty() { disabled = t[3].to_string(); }
                if t.len() >= 5 && !t[4].is_empty() { mirror = t[4].to_string(); }
                if t.len() >= 6 && !t[5].is_empty() { position = t[5].to_string(); }
                if t.len() >= 7 && !t[6].is_empty() { mode = t[6].to_string(); }
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

fn delegate(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, out, err) = run("monitors", &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        return "ERR monitors: missing subcommand".to_string();
    }
    match args[0].as_str() {
        "status" => {
            let json = hypr_monitors(true);
            let filter = r#". as $all | ($all | map({ key: (.id | tostring), value: .name }) | from_entries) as $names | $all[] | "\(.name)\t\(.width)x\(.height)\tscale \(.scale)\t" + (if .disabled then "off" elif (.mirrorOf // "none") == "none" or (.mirrorOf // "") == "" then "extended" else "mirror of \($names[.mirrorOf] // .mirrorOf)" end) + (if .focused then "\tfocused" else "" end)"#;
            jq_filter(&json, filter).trim_end().to_string()
        }
        "list" => hypr_monitors(true).trim().to_string(),
        "set" => {
            if args.len() < 4 {
                return "ERR usage: monitors set OUTPUT KEY VALUE".to_string();
            }
            let output = &args[1]; let key = &args[2]; let value = &args[3];
            if let Err(e) = apply_change(output, key, value) { return format!("ERR {}", e); }
            if let Err(e) = save_field(output, key, value) { return format!("ERR {}", e); }
            format!("OK set {} {} {}", output, key, value)
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
                    else { if idx > 0 { idx -= 1; } }
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
            // sync/show/reset: delegate (complex workspace block logic)
            delegate(args)
        }
        _ => format!("ERR unknown monitors subcommand: {}", args[0]),
    }
}
// ---------- helpers ----------

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
    // delegate count via jq
    let json = hypr_monitors(true);
    let filter = r#"[.[] | select(.disabled == false)] | length"#;
    jq_filter(&json, filter).trim().parse().unwrap_or(0)
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

fn try_action(args: &[String]) -> String {
    // args: [output, key, value]
    if args.len() < 3 { return "ERR try needs an output, a setting and a value".to_string(); }
    let output = &args[0]; let key = &args[1]; let value = &args[2];
    if !output.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return format!("ERR invalid output name: {}", output);
    }
    let pending = state_dir().join("display-pending");
    if pending.exists() { return "ERR a display change is already pending; run keep or revert first".to_string(); }
    if field_of(output, "name").is_empty() { return format!("ERR no output called \"{}\"", output); }
    if field_of(output, "disabled") == "true" { return format!("ERR {} is off; turn it on first", output); }

    let st = match read_state(output) { Some(s) => s, None => return format!("ERR no output called \"{}\"", output) };
    let old = match key.as_str() {
        "scale" => {
            // validate
            let n: f64 = match value.parse() { Ok(v) => v, Err(_) => return "ERR scale must be one of ladder".to_string() };
            if n <= 0.0 { return "ERR scale must be > 0".to_string(); }
            // check ladder
            let ladder = ["1","1.25","1.5","1.6","2","3","4"];
            if !ladder.contains(&value.as_str()) { return "ERR scale must be one of: 1 1.25 1.5 1.6 2 3 4".to_string(); }
            st.0.clone()
        }
        "transform" => {
            match transform_index(value) { Some(_) => {}, None => return "ERR transform must be one of: normal 90 180 270 flipped flipped-90 flipped-180 flipped-270".to_string() }
            st.1.clone()
        }
        "mode" => {
            let v = value.trim_end_matches("Hz");
            match v {
                "preferred" | "highres" | "highrr" | "maxwidth" => {}
                _ => {
                    // check WxH[@R]
                    if !v.chars().all(|c| c.is_digit(10) || c=='x' || c=='@' || c=='.') { return "ERR mode must be preferred, highres, highrr, or WxH[@R]".to_string(); }
                }
            }
            st.5.clone()
        }
        "position" => {
            match value.as_str() {
                "auto" | "auto-left" | "auto-right" | "auto-up" | "auto-down" => {}
                _ => {
                    if !value.chars().all(|c| c.is_digit(10) || c=='x' || c=='-') { return "ERR position must be auto, auto-left/right/up/down, or XxY".to_string(); }
                }
            }
            st.4.clone()
        }
        _ => return "ERR try takes scale, transform, mode or position".to_string(),
    };

    // write pending file: output \t key \t old \t value
    let _ = std::fs::create_dir_all(state_dir());
    let tmp = format!("/tmp/monitors.{}", std::process::id());
    let content = format!("{}\t{}\t{}\t{}\n", output, key, old, value);
    if std::fs::write(&tmp, &content).is_err() { return "ERR could not write pending".to_string(); }
    if std::fs::rename(&tmp, &pending).is_err() { let _ = std::fs::remove_file(&tmp); return "ERR could not write pending".to_string(); }

    // arm timer: systemd-run --user --on-active=15s alexie monitors revert
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

    // apply change
    let apply_val = if key == "transform" {
        transform_index(value).unwrap().to_string()
    } else if key == "mode" {
        value.trim_end_matches("Hz").to_string()
    } else {
        value.clone()
    };
    if let Err(e) = apply_change(output, key, &apply_val) { return format!("ERR {}", e); }

    format!("{} {} {} (reverting in 15s unless kept)", output, key, value)
}

fn keep_action() -> String {
    let pending = state_dir().join("display-pending");
    if !pending.exists() { return "no display change to keep".to_string(); }
    let content = match std::fs::read_to_string(&pending) { Ok(s) => s, Err(_) => return "ERR could not read pending".to_string() };
    let mut parts = content.trim().split('\t');
    let output = parts.next().unwrap_or("").to_string();
    let key = parts.next().unwrap_or("").to_string();
    let _old = parts.next().unwrap_or("").to_string();
    let new = parts.next().unwrap_or("").to_string();
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
    let old = parts.next().unwrap_or("").to_string();
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
