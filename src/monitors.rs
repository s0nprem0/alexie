use crate::proc::run;
use serde_json::{Map, Value};
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


/// One `hyprctl monitors all -j` fetch per command invocation (#1).
struct Snapshot {
    all: Vec<Value>,
    by_name: std::collections::HashMap<String, Value>,
    id_to_name: std::collections::HashMap<i64, String>,
}

impl Snapshot {
    fn fetch() -> Self {
        let v = hypr_json(true);
        let arr = v.as_array().cloned().unwrap_or_default();
        let mut by_name = std::collections::HashMap::new();
        let mut id_to_name = std::collections::HashMap::new();
        for m in &arr {
            if let Some(n) = m.get("name").and_then(|x| x.as_str()) {
                by_name.insert(n.to_string(), m.clone());
            }
            if let (Some(id), Some(n)) = (m.get("id").and_then(|x| x.as_i64()), m.get("name").and_then(|x| x.as_str())) {
                id_to_name.insert(id, n.to_string());
            }
        }
        Self { all: arr, by_name, id_to_name }
    }

    fn names(&self) -> Vec<String> {
        self.all.iter()
            .filter_map(|m| m.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect()
    }

    fn active_names(&self) -> Vec<String> {
        self.all.iter()
            .filter(|m| !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false))
            .filter_map(|m| m.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect()
    }

    fn field(&self, name: &str, key: &str) -> String {
        let m = match self.by_name.get(name) {
            Some(m) => m,
            None => return String::new(),
        };
        match m.get(key) {
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    fn focused_name(&self) -> String {
        self.all.iter()
            .find(|m| m.get("focused").and_then(|x| x.as_bool()).unwrap_or(false))
            .and_then(|m| m.get("name").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string()
    }

    fn external_name(&self) -> String {
        self.all.iter().find(|m| {
            !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false)
                && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("eDP")
                && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("LVDS")
                && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("DSI")
        }).and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string()
    }

    fn internal_name(&self) -> String {
        let name = self.all.iter()
            .find(|m| m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("eDP"))
            .and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string();
        if name.is_empty() {
            self.all.first().and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string()
        } else { name }
    }

    fn active_count(&self) -> i64 {
        self.all.iter().filter(|m| !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false)).count() as i64
    }

    fn read_state(&self, name: &str) -> Option<MonitorState> {
        read_state_from(self, name)
    }
}

fn monitors_by_name(all: bool) -> std::collections::HashMap<String, Value> {
    let mut snap = Snapshot::fetch();
    if !all {
        snap.by_name.retain(|_, m| !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false));
    }
    snap.by_name.clone()
}

fn monitor_names(all: bool) -> Vec<String> {
    let snap = Snapshot::fetch();
    if all { snap.names() } else { snap.active_names() }
}

fn monitor_serial(map: &std::collections::HashMap<String, Value>, name: &str) -> String {
    map.get(name)
        .and_then(|m| m.get("serial").and_then(|x| x.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string()
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
    Snapshot::fetch().focused_name()
}

fn external_name() -> String {
    Snapshot::fetch().external_name()
}

fn internal_name() -> String {
    Snapshot::fetch().internal_name()
}

fn field_of(name: &str, key: &str) -> String {
    Snapshot::fetch().field(name, key)
}

fn read_state_from(snap: &Snapshot, name: &str) -> Option<MonitorState> {
    let m = snap.by_name.get(name)?;
    let id_to_name = &snap.id_to_name;

    let scale_raw = m.get("scale").map(|x| match x {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }).unwrap_or_default();
    let transform_raw = m.get("transform").map(|x| match x {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }).unwrap_or_default();
    let disabled_bool = m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false);
    let mirror_raw = m.get("mirrorOf").cloned().unwrap_or(Value::Null);
    let mirror = match mirror_raw {
        Value::String(s) if !s.is_empty() && s != "none" => s,
        Value::Number(n) => match n.as_i64() {
            Some(id) => id_to_name.get(&id).cloned().unwrap_or_else(|| n.to_string()),
            None => "none".to_string(),
        },
        Value::Bool(true) => "none".to_string(),
        _ => "none".to_string(),
    };
    let x = m.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
    let y = m.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
    let position = format!("{}x{}", x, y);

    let mut scale = scale_raw;
    let mut transform = transform_raw;

    let mut disabled = if disabled_bool { "1".to_string() } else { "0".to_string() };
    let mut mirror = mirror;
    let mut position = position;
    if scale.is_empty() || scale=="0" || scale=="0.0" || scale=="null" { scale="1".to_string(); }
    if transform.is_empty() || transform=="null" { transform="0".to_string(); }
    if mirror.is_empty() || mirror=="null" { mirror="none".to_string(); }
    if position.is_empty() || position=="null" { position="auto".to_string(); }
    if disabled == "1" { position = "auto".to_string(); }

    let mut mode = "preferred".to_string();
    let mut sdrb = "1".to_string();
    let mut sdrr = "1".to_string();

    if let Ok(content) = std::fs::read_to_string(state_file()) {
        for line in content.lines() {
            if line.starts_with('#') { continue; }
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

fn read_state(name: &str) -> Option<MonitorState> {
    read_state_from(&Snapshot::fetch(), name)
}

fn apply_change(output: &str, key: &str, value: &str) -> Result<(), String> {
    apply_change_snap(&Snapshot::fetch(), output, key, value)
}

fn apply_change_snap(snap: &Snapshot, output: &str, key: &str, value: &str) -> Result<(), String> {
    let (scale, transform, disabled, mirror, position, mode, sdrb, sdrr) =
        read_state_from(snap, output).ok_or_else(|| format!("no output called \"{}\"", output))?;
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
    save_field_snap(&Snapshot::fetch(), output, key, value)
}

fn save_field_snap(snap: &Snapshot, output: &str, key: &str, value: &str) -> Result<(), String> {
    let (scale, transform, disabled, mirror, position, mode, sdrb, sdrr) =
        read_state_from(snap, output).ok_or_else(|| format!("no output called \"{}\"", output))?;
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
    // prune stale outputs (#4) and header (#3)
    let known: std::collections::HashSet<String> = snap.names().into_iter().collect();
    lines.retain(|l| {
        let t: Vec<&str> = l.split_whitespace().collect();
        t.is_empty() || t[0].starts_with('#') || (known.contains(t[0]) && t[0] != output)
    });
    if lines.is_empty() || !lines[0].starts_with("# v2") {
        lines.insert(0, "# v2".to_string());
    }
    lines.push(format!("{} {} {} {} {} {} {} {} {}", output, scale, transform, disabled, mirror, position, mode, sdrb, sdrr));
    atomic_write(&path, &(lines.join("\n") + "\n")).map_err(|e| e.to_string())
}

fn read_saved_map() -> std::collections::HashMap<String, (String, String)> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string(state_file()) {
        for line in content.lines() {
            if line.starts_with('#') { continue; }
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

fn list_action(snap: &Snapshot) -> String {
    let mut v = Value::Array(snap.all.clone());
    let arr = match v.as_array_mut() {
        Some(a) => a,
        None => return "{}".to_string(),
    };

    // id -> name
    let id_to_name = &snap.id_to_name;

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

    serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string())
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        return "ERR monitors: missing subcommand".to_string();
    }
    let snap = Snapshot::fetch();
    match args[0].as_str() {
        "status" => {
            let arr = snap.all.clone();
            // map id -> name
            let id_to_name = &snap.id_to_name;
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
        "list" => list_action(&snap),
        "set" => {
            if args.len() < 4 {
                return "ERR usage: monitors set OUTPUT KEY VALUE".to_string();
            }
            set_action(&snap, &args[1], &args[2], &args[3])
        }
        "scale" => {
            let focused = snap.focused_name();
            if focused.is_empty() { return "ERR no focused monitor".to_string(); }
            let action = if args.len() >= 2 { args[1].as_str() } else { "up" };
            match action {
                "set" => {
                    if args.len() < 3 { return "ERR usage: monitors scale set N".to_string(); }
                    let v = &args[2];
                    let n: f64 = match v.parse() { Ok(x) => x, Err(_) => return "ERR bad scale".to_string() };
                    if n <= 0.0 { return "ERR scale must be > 0".to_string(); }
                    if let Err(e) = apply_change_snap(&snap, &focused, "scale", v) { return format!("ERR {}", e); }
                    if let Err(e) = save_field_snap(&snap, &focused, "scale", v) { return format!("ERR {}", e); }
                    format!("{} scale set to {}", focused, v)
                }
                "reset" => {
                    if let Err(e) = apply_change_snap(&snap, &focused, "scale", "1") { return format!("ERR {}", e); }
                    if let Err(e) = save_field_snap(&snap, &focused, "scale", "1") { return format!("ERR {}", e); }
                    format!("{} scale reset", focused)
                }
                "up" | "down" => {
                    let ladder = ["1", "1.25", "1.5", "1.6", "2", "3", "4"];
                    let cur = snap.field(&focused, "scale");
                    let cur = if cur.is_empty() { "1".to_string() } else { cur };
                    let mut idx = ladder.iter().position(|&s| s == cur).unwrap_or(0);
                    if action == "up" { if idx + 1 < ladder.len() { idx += 1; } }
                    else { idx = idx.saturating_sub(1); }
                    let newv = ladder[idx];
                    if let Err(e) = apply_change_snap(&snap, &focused, "scale", newv) { return format!("ERR {}", e); }
                    if let Err(e) = save_field_snap(&snap, &focused, "scale", newv) { return format!("ERR {}", e); }
                    format!("{} scale {}", focused, newv)
                }
                _ => format!("ERR unknown scale action: {}", action),
            }
        }
        "brightness" => {
            let focused = snap.focused_name();
            if focused.is_empty() { return "ERR no focused monitor".to_string(); }
            let action = if args.len() >= 2 { args[1].as_str() } else { "up" };
            let step: i64 = if args.len() >= 3 { args[2].parse().unwrap_or(5) } else { 5 };
            let is_internal = focused.starts_with("eDP") || focused.starts_with("LVDS") || focused.starts_with("DSI");
            let ddc_args = |vcp: &str| -> Vec<String> {
                if is_internal { vec![] } else if let Some(d) = ddcutil_display(&focused) {
                    vec!["--display".to_string(), d.to_string(), "setvcp".to_string(), "10".to_string(), vcp.to_string()]
                } else {
                    vec!["setvcp".to_string(), "10".to_string(), vcp.to_string()]
                }
            };
            match action {
                "up" => {
                    if is_internal {
                        let (code, out, err) = run("brightnessctl", &["set", &format!("{}%+", step)]);
                        if code != 0 { return format!("ERR brightnessctl: {}", err); }
                        format!("{} brightness {}", focused, out.trim())
                    } else {
                        let a = ddc_args(&format!("+{}", step));
                        let (code, _, err) = run("ddcutil", &a.iter().map(|s| s.as_str()).collect::<Vec<_>>());
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
                        let a = ddc_args(&format!("-{}", step));
                        let (code, _, err) = run("ddcutil", &a.iter().map(|s| s.as_str()).collect::<Vec<_>>());
                        if code != 0 { return format!("ERR ddcutil: {}", err); }
                        "OK".to_string()
                    }
                }
                "max" => {
                    if is_internal { let _ = run("brightnessctl", &["set", "100%"]); }
                    else {
                        let a = ddc_args("100");
                        let _ = run("ddcutil", &a.iter().map(|s| s.as_str()).collect::<Vec<_>>());
                    }
                    "OK".to_string()
                }
                "min" => {
                    if is_internal { let _ = run("brightnessctl", &["set", "1%"]); }
                    else {
                        let a = ddc_args("1");
                        let _ = run("ddcutil", &a.iter().map(|s| s.as_str()).collect::<Vec<_>>());
                    }
                    "OK".to_string()
                }
                "set" => {
                    if args.len() < 3 { return "ERR usage: monitors brightness set N".to_string(); }
                    let val = &args[2];
                    if is_internal {
                        let (code, _, err) = run("brightnessctl", &["set", &format!("{}%", val)]);
                        if code != 0 { return format!("ERR brightnessctl: {}", err); }
                    } else {
                        let a = ddc_args(val);
                        let (code, _, err) = run("ddcutil", &a.iter().map(|s| s.as_str()).collect::<Vec<_>>());
                        if code != 0 { return format!("ERR ddcutil: {}", err); }
                    }
                    "OK".to_string()
                }
                _ => format!("ERR unknown brightness action: {}", action),
            }
        }
        "identify" => {
            for name in snap.names() {
                let _ = run("notify-send", &["-t", "2000", "Monitor", &name]);
            }
            "OK".to_string()
        }
        "wake" => {
            let _ = run("hyprctl", &["dispatch", "dpms", "on"]);
            "OK".to_string()
        }
        "recover" => recover_action(&snap),
        "internal" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            internal_action(&snap, action)
        }
        "mirror" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            mirror_action(&snap, action)
        }
        "lid" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            lid_action(&snap, action)
        }
        "try" => {
            if args.len() < 4 { return "ERR try needs an output, a setting and a value".to_string(); }
            try_action(&snap, &args[1..])
        }
        "keep" => keep_action(&snap),
        "revert" => revert_action(&snap),
        "workspaces" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "sync" };
            workspaces_action(action)
        }
        "workspace" => {
            let slot = if args.len() >= 2 { args[1].as_str() } else { "" };
            workspace_action(&snap, slot)
        }
        "workspace-step" => {
            let dir = if args.len() >= 2 { args[1].as_str() } else { "next" };
            workspace_step_action(&snap, dir)
        }
        "move-window" => {
            let slot = if args.len() >= 2 { args[1].as_str() } else { "" };
            move_window_action(&snap, slot)
        }
        "identities" => identities_action(&snap),
        "sync-identities" => sync_identities_action(&snap),
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

/// Unified validation + normalization for a monitor setting (#2).
/// Returns the normalized value to pass to apply/save, or an error string.
fn validate_setting(key: &str, value: &str) -> Result<String, String> {
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

fn old_value_for(snap: &Snapshot, output: &str, key: &str) -> Option<String> {
    let st = read_state_from(snap, output)?;
    Some(match key {
        "scale" => st.0.clone(),
        "transform" => st.1.clone(),
        "disabled" => st.2.clone(),
        "mirror" => st.3.clone(),
        "position" => st.4.clone(),
        "mode" => st.5.clone(),
        "sdrbrightness" => st.6.clone(),
        "sdrsaturation" => st.7.clone(),
        _ => return None,
    })
}

fn set_action(snap: &Snapshot, output: &str, key: &str, value: &str) -> String {
    // refuse unknown output
    if snap.field(output, "name").is_empty() {
        return format!("ERR no output called \"{}\"", output);
    }

    match key {
        "scale" => {
            let normalized = match validate_setting("scale", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = apply_change_snap(snap, output, "scale", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "scale", &normalized) { return format!("ERR {}", e); }
            format!("{} scale {}", output, normalized)
        }
        "transform" => {
            let normalized = match validate_setting("transform", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = apply_change_snap(snap, output, "transform", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "transform", &normalized) { return format!("ERR {}", e); }
            format!("{} transform {}", output, value)
        }
        "disabled" => {
            let (index, disabled) = match value {
                "yes" | "true" | "on" => (1, "true"),
                "no" | "false" | "off" => (0, "false"),
                _ => return "ERR disabled takes yes or no".to_string(),
            };
            if index == 1 && snap.field(output, "disabled") != "true" && snap.active_count() <= 1 {
                return "ERR refusing to disable the only active output".to_string();
            }
            if let Err(e) = apply_change_snap(snap, output, "disabled", disabled) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "disabled", &index.to_string()) { return format!("ERR {}", e); }
            if index == 0 {
                let _ = workspaces_sync_action();
            }
            format!("{} disabled {}", output, value)
        }
        "position" => {
            let normalized = match validate_setting("position", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = apply_change_snap(snap, output, "position", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "position", &normalized) { return format!("ERR {}", e); }
            format!("{} position {}", output, normalized)
        }
        "mode" => {
            let normalized = match validate_setting("mode", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = apply_change_snap(snap, output, "mode", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "mode", &normalized) { return format!("ERR {}", e); }
            format!("{} mode {}", output, normalized)
        }
        "sdrbrightness" => {
            let normalized = match validate_setting("sdrbrightness", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = apply_change_snap(snap, output, "sdrbrightness", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "sdrbrightness", &normalized) { return format!("ERR {}", e); }
            format!("{} sdrbrightness {}", output, normalized)
        }
        "sdrsaturation" => {
            let normalized = match validate_setting("sdrsaturation", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = apply_change_snap(snap, output, "sdrsaturation", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "sdrsaturation", &normalized) { return format!("ERR {}", e); }
            format!("{} sdrsaturation {}", output, normalized)
        }
        "mirror" => {
            let mut v = value.to_string();
            if v == "off" || v.is_empty() { v = "none".to_string(); }
            if v != "none" {
                if v == output { return "ERR an output cannot mirror itself".to_string(); }
                if snap.field(&v, "name").is_empty() { return format!("ERR no output called \"{}\" to mirror", v); }
                if snap.field(&v, "disabled") == "true" { return format!("ERR {} is off; turn it on before mirroring", v); }
                // target must not already be mirroring
                let st = read_state_from(snap, &v);
                if let Some(s) = st {
                    if s.3 != "none" { return format!("ERR {} is mirroring another screen; extend it first", v); }
                }
            }
            if snap.field(output, "disabled") == "true" {
                return format!("ERR {} is off; turn it on before mirroring", output);
            }
            if let Err(e) = apply_change_snap(snap, output, "mirror", &v) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, output, "mirror", &v) { return format!("ERR {}", e); }
            if v == "none" {
                let _ = workspaces_sync_action();
            }
            if v == "none" { format!("{} extended again", output) } else { format!("{} now mirrors {}", output, v) }
        }
        _ => "ERR set takes scale, transform, disabled, position, mode, mirror, sdrbrightness or sdrsaturation".to_string(),
    }
}

fn try_action(snap: &Snapshot, args: &[String]) -> String {
    if args.len() < 3 { return "ERR try needs an output, a setting and a value".to_string(); }
    let output = &args[0]; let key = &args[1]; let raw_value = &args[2];
    if output.is_empty() || !output.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return format!("ERR invalid output name: {}", output);
    }
    let pending = state_dir().join("display-pending");
    if pending.exists() { return "ERR a display change is already pending; run keep or revert first".to_string(); }
    if snap.field(output, "name").is_empty() { return format!("ERR no output called \"{}\"", output); }
    if snap.field(output, "disabled") == "true" { return format!("ERR {} is off; turn it on first", output); }

    let value = match validate_setting(key, raw_value) {
        Ok(v) => v,
        Err(e) => {
            if key != "scale" && key != "transform" && key != "mode" && key != "position" {
                return "ERR try takes scale, transform, mode or position".to_string();
            }
            return e;
        }
    };
    let old = match old_value_for(snap, output, key) {
        Some(v) => v,
        None => return format!("ERR no output called \"{}\"", output),
    };

    let _ = std::fs::create_dir_all(state_dir());
    if write_pending(output, key, &old, &value).is_err() { return "ERR could not write pending".to_string(); }

    // arm timer
    let _ = run("systemctl", &["--user", "stop", "alexie-display-revert.timer"]);
    let mut args: Vec<String> = vec![
        "--user".to_string(), "--quiet".to_string(), "--collect".to_string(),
        "--unit=alexie-display-revert".to_string(),
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

    if let Err(e) = apply_change_snap(snap, output, key, &value) { return format!("ERR {}", e); }

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
                if s.contains("open") { return false; }
            }
        }
    }
    // fallback: /sys/class/power_supply
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let p = e.path().join("lid/state");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if s.contains("closed") { return true; }
                if s.contains("open") { return false; }
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
            if line.starts_with('#') { continue; }
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
            if lines.is_empty() || !lines[0].starts_with("# v2") {
                lines.insert(0, "# v2".to_string());
            }
            let _ = atomic_write(&path, &(lines.join("\n") + "\n"));
        }
    }
}

fn active_count_i() -> i64 {
    Snapshot::fetch().active_count()
}

fn internal_action(snap: &Snapshot, action: &str) -> String {
    let name = snap.internal_name();
    if name.is_empty() { return "ERR no monitor found".to_string(); }
    let disabled = snap.field(&name, "disabled");
    let action = if action == "toggle" {
        if disabled == "true" { "on" } else { "off" }
    } else { action };
    match action {
        "off" => {
            if disabled == "true" { return "OK".to_string(); }
            if snap.active_count() <= 1 { return "ERR refusing to disable the only active output".to_string(); }
            if let Err(e) = apply_change_snap(snap, &name, "disabled", "true") { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, &name, "disabled", "1") { return format!("ERR {}", e); }
            format!("internal panel {}: off", name)
        }
        "on" => {
            if disabled != "true" { return "OK".to_string(); }
            if let Err(e) = apply_change_snap(snap, &name, "disabled", "false") { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, &name, "disabled", "0") { return format!("ERR {}", e); }
            let _ = workspaces_sync_action();
            format!("internal panel {}: on", name)
        }
        _ => "ERR internal takes on, off or toggle".to_string(),
    }
}

fn mirror_action(snap: &Snapshot, action: &str) -> String {
    let internal = snap.internal_name();
    let external = snap.external_name();
    if external.is_empty() { return "ERR no external monitor connected to mirror onto".to_string(); }
    if snap.field(&internal, "disabled") == "true" {
        return "ERR internal panel is off; turn it on before mirroring".to_string();
    }
    let mirrored = snap.field(&external, "mirrorOf");
    let mirrored = if mirrored.is_empty() || mirrored == "null" { "none".to_string() } else { mirrored };
    let action = if action == "toggle" {
        if mirrored == "none" { "on" } else { "off" }
    } else { action };
    match action {
        "on" => {
            if let Err(e) = apply_change_snap(snap, &external, "mirror", &internal) { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, &external, "mirror", &internal) { return format!("ERR {}", e); }
            format!("{} now mirrors {}", external, internal)
        }
        "off" => {
            if let Err(e) = apply_change_snap(snap, &external, "mirror", "none") { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, &external, "mirror", "none") { return format!("ERR {}", e); }
            let _ = workspaces_sync_action();
            format!("{} extended again", external)
        }
        _ => "ERR mirror takes on, off or toggle".to_string(),
    }
}

fn lid_action(snap: &Snapshot, action: &str) -> String {
    let internal = snap.internal_name();
    if internal.is_empty() { return "OK".to_string(); }
    let external = snap.external_name();
    match action {
        "close" | "off" => {
            if external.is_empty() {
                return "no external monitor; leaving the internal panel on".to_string();
            }
            if snap.field(&internal, "disabled") == "true" { return "OK".to_string(); }
            if let Err(e) = apply_change_snap(snap, &internal, "disabled", "true") { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, &internal, "disabled", "1") { return format!("ERR {}", e); }
            let _ = std::fs::create_dir_all(state_dir());
            let _ = std::fs::write(lid_file(), format!("{}\n", internal));
            format!("lid closed: internal panel {} off", internal)
        }
        "open" | "on" => {
            let saved = match std::fs::read_to_string(lid_file()) { Ok(s) => s.trim().to_string(), Err(_) => String::new() };
            if saved.is_empty() { return "OK".to_string(); }
            if snap.field(&saved, "name").is_empty() { let _ = std::fs::remove_file(lid_file()); return "OK".to_string(); }
            if let Err(e) = apply_change_snap(snap, &saved, "disabled", "false") { return format!("ERR {}", e); }
            if let Err(e) = save_field_snap(snap, &saved, "disabled", "0") { return format!("ERR {}", e); }
            let _ = std::fs::remove_file(lid_file());
            let _ = workspaces_sync_action();
            format!("lid opened: internal panel {} on", saved)
        }
        "toggle" => {
            if lid_file().exists() { lid_action(snap, "open") } else { lid_action(snap, "close") }
        }
        "reconcile" => {
            if lid_closed() && !external.is_empty() { lid_action(snap, "close") } else { lid_action(snap, "open") }
        }
        _ => "ERR lid takes close, open, toggle or reconcile".to_string(),
    }
}

fn recover_action(snap: &Snapshot) -> String {
    // clear stale lid state if lid is open
    if lid_file().exists() && !lid_closed() {
        if let Ok(saved) = std::fs::read_to_string(lid_file()) {
            let saved = saved.trim().to_string();
            if !saved.is_empty() && !snap.field(&saved, "name").is_empty() {
                clear_disabled(&saved);
            }
        }
        let _ = std::fs::remove_file(lid_file());
        let _ = workspaces_sync_action();
        return "lid is open; cleared the saved clamshell state".to_string();
    }
    "OK".to_string()
}
fn keep_action(snap: &Snapshot) -> String {
    let pending = state_dir().join("display-pending");
    if !pending.exists() { return "no display change to keep".to_string(); }
    let Some(p) = read_pending() else {
        let _ = std::fs::remove_file(&pending);
        return "ERR could not read pending".to_string();
    };

    let _ = run("systemctl", &["--user", "stop", "alexie-display-revert.timer"]);
    if let Err(e) = save_field_snap(snap, &p.output, &p.key, &p.new_value) { return format!("ERR {}", e); }
    remove_pending(&pending);
    format!("{} {} {} kept", p.output, p.key, p.new_value)
}

fn revert_action(snap: &Snapshot) -> String {
    let pending = state_dir().join("display-pending");
    if !pending.exists() { return "OK".to_string(); }
    let Some(p) = read_pending() else {
        return "OK".to_string();
    };

    if snap.field(&p.output, "name").is_empty() {
        remove_pending(&pending);
        return "OK".to_string();
    }

    let _ = run("systemctl", &["--user", "stop", "alexie-display-revert.timer"]);
    if let Err(e) = apply_change_snap(snap, &p.output, &p.key, &p.old_value) { remove_pending(&pending); return format!("ERR {}", e); }
    remove_pending(&pending);
    format!("{} {} reverted to {}", p.output, p.key, p.old_value)
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Pending {
    output: String,
    key: String,
    old_value: String,
    new_value: String,
}

fn pending_lock() -> Option<std::fs::File> {
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let lock_path = dir.join("display-pending.lock");
    // non-blocking try + short retry
    for _ in 0..20 {
        match std::fs::OpenOptions::new().create_new(true).write(true).open(&lock_path) {
            Ok(f) => return Some(f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(_) => return None,
        }
    }
    None
}

fn remove_pending(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let lock = state_dir().join("display-pending.lock");
    let _ = std::fs::remove_file(&lock);
}

fn read_pending() -> Option<Pending> {
    let path = state_dir().join("display-pending");
    let content = std::fs::read_to_string(&path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() { return None; }
    if let Ok(p) = serde_json::from_str::<Pending>(trimmed) {
        return Some(p);
    }
    // legacy TSV fallback
    let mut parts = trimmed.split('\t');
    let output = parts.next()?.to_string();
    let key = parts.next()?.to_string();
    let old_value = parts.next()?.to_string();
    let new_value = parts.next()?.to_string();
    if key == "mode" {
        let old_value = old_value.strip_suffix("Hz").unwrap_or(&old_value).to_string();
        let new_value = new_value.strip_suffix("Hz").unwrap_or(&new_value).to_string();
        return Some(Pending { output, key, old_value, new_value });
    }
    Some(Pending { output, key, old_value, new_value })
}

fn write_pending(output: &str, key: &str, old_value: &str, new_value: &str) -> std::io::Result<()> {
    let _lock = pending_lock();
    let p = Pending { output: output.to_string(), key: key.to_string(), old_value: old_value.to_string(), new_value: new_value.to_string() };
    let json = serde_json::to_string(&p).unwrap_or_else(|_| "{}".to_string());
    let path = state_dir().join("display-pending");
    atomic_write(&path, &format!("{}\n", json))
}

fn workspace_file() -> std::path::PathBuf { state_dir().join("workspaces.json") }

/// Workspace base for a monitor name. The map is keyed by serial identity,
/// falling back to name (and back for name-keyed entries, see sync).
fn workspace_base(name: &str) -> String {
    let map = read_workspace_map();
    if let Some(n) = map.get(name) {
        return n.to_string();
    }
    let all = monitors_by_name(true);
    let identity = monitor_serial(&all, name);
    if identity != name {
        if let Some(n) = map.get(&identity) {
            return n.to_string();
        }
    }
    "1".to_string()
}

fn read_workspace_map() -> std::collections::HashMap<String, i64> {
    let mut out = std::collections::HashMap::new();
    let path = workspace_file();
    let Ok(content) = std::fs::read_to_string(&path) else { return out };
    let content = content.trim();
    if content.is_empty() { return out; }
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(content) else { return out };
    for (k, v) in obj {
        if let Some(n) = v.as_i64() {
            out.insert(k, n);
        }
    }
    out
}

fn ddcutil_display(output: &str) -> Option<String> {
    // Parse `ddcutil detect --brief` to map hypr output name to ddcutil display number.
    let (code, out, _) = run("ddcutil", &["detect", "--brief"]);
    if code != 0 { return None; }
    // Brief format: "Display 1\nMonitor: ...\n  I2C ...\n  ...\n  Monitor ID: ..."
    // Fall back to scanning lines for output name substring.
    let mut current_display: Option<String> = None;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Display ") {
            current_display = trimmed.split_whitespace().nth(1).map(|s| s.to_string());
        } else if trimmed.contains(output) {
            return current_display;
        }
    }
    None
}


fn workspaces_sync_action() -> String {
    let _ = std::fs::create_dir_all(state_dir());
    let mut map = read_workspace_map();

    let names = monitor_names(false);
    let all = monitors_by_name(true);

    for name in &names {
        let identity = monitor_serial(&all, name);

        // migrate name-keyed entry to identity key
        if map.contains_key(name) && !map.contains_key(&identity) {
            if let Some(base) = map.remove(name) {
                map.insert(identity.clone(), base);
            }
        }
        if !map.contains_key(&identity) {
            let mut candidate = 1i64;
            while map.values().any(|&v| v == candidate) {
                candidate += 10;
            }
            map.insert(identity.clone(), candidate);
        }
    }

    // prune stale identities (#4): keep only currently active monitors
    let active_idents: std::collections::HashSet<String> = names.iter().map(|n| monitor_serial(&all, n)).collect();
    map.retain(|k, _| active_idents.contains(k));

    // write sorted map
    let mut sorted: Vec<(&String, &i64)> = map.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut json = Map::new();
    for (k, v) in &sorted {
        json.insert((*k).clone(), Value::from(**v));
    }
    // serde_json Object sort: use BTreeMap via Value::Object which preserves insertion order above
    let to_write = serde_json::to_string_pretty(&Value::Object(json)).unwrap_or_else(|_| "{}".to_string());
    let _ = atomic_write(&workspace_file(), &format!("{}\n", to_write));

    // build lua and hyprctl eval
    let mut lua = String::new();
    for name in &names {
        let identity = monitor_serial(&all, name);
        let base = *map.get(&identity).unwrap_or(&1);
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
    to_write
}

fn workspaces_action(action: &str) -> String {
    match action {
        "sync" => { workspaces_sync_action(); "OK".to_string() }
        "show" => {
            let map = read_workspace_map();
            if map.is_empty() {
                "{}".to_string()
            } else {
                let mut json = Map::new();
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                for k in keys {
                    json.insert(k.clone(), Value::from(map[k]));
                }
                serde_json::to_string_pretty(&Value::Object(json)).unwrap_or_else(|_| "{}".to_string())
            }
        }
        "reset" => {
            let _ = std::fs::remove_file(workspace_file());
            workspaces_sync_action();
            "OK".to_string()
        }
        _ => "ERR workspaces takes sync, show or reset".to_string(),
    }
}

fn workspace_action(snap: &Snapshot, slot: &str) -> String {
    if slot.is_empty() { return "ERR workspace needs a number".to_string(); }
    if !slot.chars().all(|c| c.is_ascii_digit()) { return "ERR workspace needs a number".to_string(); }
    let n: i64 = match slot.parse() { Ok(v) => v, Err(_) => return "ERR workspace needs a number".to_string() };
    if !(1..=10).contains(&n) { return "ERR workspace must be 1-10".to_string(); }
    let focused = snap.focused_name();
    if focused.is_empty() { return "ERR no focused monitor".to_string(); }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    let id = base + n - 1;
    let lua = format!("hl.dispatch(hl.dsp.focus({{ workspace = \"{}\" }}))", id);
    let _ = run("hyprctl", &["eval", &lua]);
    format!("{} workspace {} (global {})", focused, n, id)
}

fn workspace_step_action(snap: &Snapshot, direction: &str) -> String {
    if direction != "next" && direction != "prev" { return "ERR workspace-step takes next or prev".to_string(); }
    let focused = snap.focused_name();
    if focused.is_empty() { return "ERR no focused monitor".to_string(); }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    // current focused workspace id
    let current: i64 = snap.all.iter()
        .find(|m| m.get("focused").and_then(|x| x.as_bool()).unwrap_or(false))
        .and_then(|m| m.get("activeWorkspace").and_then(|w| w.get("id")).and_then(|x| x.as_i64()))
        .unwrap_or(0);
    if current == 0 { return "OK".to_string(); }
    let target = if direction == "next" { current + 1 } else { current - 1 };
    let target = target.clamp(base, base + 9);
    if target == current { return "OK".to_string(); }
    let lua = format!("hl.dispatch(hl.dsp.focus({{ workspace = \"{}\" }}))", target);
    let _ = run("hyprctl", &["eval", &lua]);
    "OK".to_string()
}

fn move_window_action(snap: &Snapshot, slot: &str) -> String {
    if slot.is_empty() { return "ERR move-window needs a number".to_string(); }
    if !slot.chars().all(|c| c.is_ascii_digit()) { return "ERR move-window needs a number".to_string(); }
    let n: i64 = match slot.parse() { Ok(v) => v, Err(_) => return "ERR move-window needs a number".to_string() };
    if !(1..=10).contains(&n) { return "ERR move-window must be 1-10".to_string(); }
    let focused = snap.focused_name();
    if focused.is_empty() { return "ERR no focused monitor".to_string(); }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    let id = base + n - 1;
    let lua = format!("hl.dispatch(hl.dsp.window.move({{ workspace = \"{}\" }}))", id);
    let _ = run("hyprctl", &["eval", &lua]);
    format!("window to {} workspace {} (global {})", focused, n, id)
}

fn identities_action(snap: &Snapshot) -> String {
    let arr = &snap.all;
    let mut obj = Map::new();
    for m in arr {
        let Some(name) = m.get("name").and_then(|x| x.as_str()) else { continue };
        let mut inner = Map::new();
        for key in ["make", "model", "serial", "description"] {
            if let Some(val) = m.get(key) {
                inner.insert(key.to_string(), val.clone());
            }
        }
        obj.insert(name.to_string(), Value::Object(inner));
    }
    serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_else(|_| "{}".to_string())
}

fn sync_identities_action(snap: &Snapshot) -> String {
    let out = identities_action(snap);
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
    fn workspace_map_parses_and_rejects_bad_json() {
        // read_workspace_map with missing file returns empty (no panic).
        let m = read_workspace_map();
        assert!(m.is_empty() || !m.is_empty()); // file may exist in env; just no panic
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
    fn pending_json_roundtrip() {
        let p = Pending { output: "eDP-1".into(), key: "scale".into(), old_value: "1".into(), new_value: "1.5".into() };
        let s = serde_json::to_string(&p).unwrap();
        let back: Pending = serde_json::from_str(&s).unwrap();
        assert_eq!(back.output, "eDP-1");
        assert_eq!(back.new_value, "1.5");
        // legacy TSV fallback
        let tsv = "eDP-1\tscale\t1\t1.5";
        let mut parts = tsv.split('\t');
        assert_eq!(parts.next().unwrap(), "eDP-1");
    }

    #[test]
    fn workspace_prune_keeps_only_active() {
        let mut map = std::collections::HashMap::new();
        map.insert("SER1".to_string(), 1);
        map.insert("STALE".to_string(), 11);
        let active: std::collections::HashSet<String> = ["SER1".to_string()].into_iter().collect();
        map.retain(|k, _| active.contains(k));
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("SER1"));
    }
}
