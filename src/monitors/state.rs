use crate::proc::run;
pub(crate) use crate::proc::quickshell as qs;
use serde_json::Value;
use std::env;

pub(crate) fn state_dir() -> std::path::PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    std::path::PathBuf::from(state).join("anixie")
}

pub(crate) type MonitorState = (String, String, String, String, String, String, String, String);

pub(crate) fn state_file() -> std::path::PathBuf { state_dir().join("display.conf") }
pub(crate) fn lid_file() -> std::path::PathBuf { state_dir().join("lid") }


// Write `content` to `dest` atomically via a temp file in the same
// directory. `/tmp` may be on a different filesystem than the state dir,
// where `std::fs::rename` fails with EXDEV; the bash original used `mv`
// which copies across devices, so do the same here.
pub(crate) fn atomic_write(dest: &std::path::Path, content: &str) -> std::io::Result<()> {
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
pub(crate) struct Snapshot {
    pub(crate) all: Vec<Value>,
    pub(crate) by_name: std::collections::HashMap<String, Value>,
    pub(crate) id_to_name: std::collections::HashMap<i64, String>,
}

impl Snapshot {
    pub(crate) fn fetch() -> Self {
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

    pub(crate) fn names(&self) -> Vec<String> {
        self.all.iter()
            .filter_map(|m| m.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect()
    }

    pub(crate) fn active_names(&self) -> Vec<String> {
        self.all.iter()
            .filter(|m| !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false))
            .filter_map(|m| m.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect()
    }

    pub(crate) fn field(&self, name: &str, key: &str) -> String {
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

    pub(crate) fn focused_name(&self) -> String {
        self.all.iter()
            .find(|m| m.get("focused").and_then(|x| x.as_bool()).unwrap_or(false))
            .and_then(|m| m.get("name").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string()
    }

    pub(crate) fn external_name(&self) -> String {
        self.all.iter().find(|m| {
            !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false)
                && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("eDP")
                && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("LVDS")
                && !m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("DSI")
        }).and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string()
    }

    pub(crate) fn internal_name(&self) -> String {
        let name = self.all.iter()
            .find(|m| m.get("name").and_then(|x| x.as_str()).unwrap_or("").starts_with("eDP"))
            .and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string();
        if name.is_empty() {
            self.all.first().and_then(|m| m.get("name").and_then(|x| x.as_str())).unwrap_or("").to_string()
        } else { name }
    }

    pub(crate) fn active_count(&self) -> i64 {
        self.all.iter().filter(|m| !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false)).count() as i64
    }

}

pub(crate) fn monitors_by_name(all: bool) -> std::collections::HashMap<String, Value> {
    let mut snap = Snapshot::fetch();
    if !all {
        snap.by_name.retain(|_, m| !m.get("disabled").and_then(|x| x.as_bool()).unwrap_or(false));
    }
    snap.by_name.clone()
}

pub(crate) fn monitor_names(all: bool) -> Vec<String> {
    let snap = Snapshot::fetch();
    if all { snap.names() } else { snap.active_names() }
}

pub(crate) fn monitor_serial(map: &std::collections::HashMap<String, Value>, name: &str) -> String {
    map.get(name)
        .and_then(|m| m.get("serial").and_then(|x| x.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string()
}

pub(crate) fn hypr_monitors(all: bool) -> String {
    let mut args = vec!["monitors"];
    if all { args.push("all"); }
    args.push("-j");
    let a: Vec<&str> = args.to_vec();
    let (_, out, _) = run("hyprctl", &a);
    out
}

pub(crate) fn hypr_json(all: bool) -> Value {
    let out = hypr_monitors(all);
    serde_json::from_str(&out).unwrap_or(Value::Array(vec![]))
}

pub fn focused_name() -> String {
    Snapshot::fetch().focused_name()
}

pub(crate) fn read_state_from(snap: &Snapshot, name: &str) -> Option<MonitorState> {
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

pub(crate) fn apply_change_snap(snap: &Snapshot, output: &str, key: &str, value: &str) -> Result<(), String> {
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

pub(crate) fn save_field_snap(snap: &Snapshot, output: &str, key: &str, value: &str) -> Result<(), String> {
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_state_defaults_for_missing_file() {
        let snap = Snapshot::fetch();
        let _ = read_state_from(&snap, "NONEXISTENT");
    }
}
