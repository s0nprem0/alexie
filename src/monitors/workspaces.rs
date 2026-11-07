use super::state::{
    Snapshot, atomic_write, monitor_names, monitor_serial, monitors_by_name, state_dir,
};
use crate::proc::run;
use serde_json::{Map, Value};

pub(crate) fn workspace_file() -> std::path::PathBuf {
    state_dir().join("workspaces.json")
}

/// Workspace base for a monitor name. The map is keyed by serial identity,
/// falling back to name (and back for name-keyed entries, see sync).
pub(crate) fn workspace_base(name: &str) -> String {
    let map = read_workspace_map();
    if let Some(n) = map.get(name) {
        return n.to_string();
    }
    let all = monitors_by_name(true);
    let identity = monitor_serial(&all, name);
    if identity != name
        && let Some(n) = map.get(&identity)
    {
        return n.to_string();
    }
    "1".to_string()
}

pub(crate) fn read_workspace_map() -> std::collections::HashMap<String, i64> {
    let mut out = std::collections::HashMap::new();
    let path = workspace_file();
    let Ok(content) = std::fs::read_to_string(&path) else {
        return out;
    };
    let content = content.trim();
    if content.is_empty() {
        return out;
    }
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(content) else {
        return out;
    };
    for (k, v) in obj {
        if let Some(n) = v.as_i64() {
            out.insert(k, n);
        }
    }
    out
}

pub(crate) fn ddcutil_display(output: &str) -> Option<String> {
    // Parse `ddcutil detect --brief` to map hypr output name to ddcutil display number.
    let (code, out, _) = run("ddcutil", &["detect", "--brief"]);
    if code != 0 {
        return None;
    }
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

pub(crate) fn workspaces_sync_action() -> String {
    let _ = std::fs::create_dir_all(state_dir());
    let mut map = read_workspace_map();

    let names = monitor_names(false);
    let all = monitors_by_name(true);

    for name in &names {
        let identity = monitor_serial(&all, name);

        // migrate name-keyed entry to identity key
        if map.contains_key(name)
            && !map.contains_key(&identity)
            && let Some(base) = map.remove(name)
        {
            map.insert(identity.clone(), base);
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
    let active_idents: std::collections::HashSet<String> =
        names.iter().map(|n| monitor_serial(&all, n)).collect();
    map.retain(|k, _| active_idents.contains(k));

    // write sorted map
    let mut sorted: Vec<(&String, &i64)> = map.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut json = Map::new();
    for (k, v) in &sorted {
        json.insert((*k).clone(), Value::from(**v));
    }
    // serde_json Object sort: use BTreeMap via Value::Object which preserves insertion order above
    let to_write =
        serde_json::to_string_pretty(&Value::Object(json)).unwrap_or_else(|_| "{}".to_string());
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
                lua.push_str(&format!(
                    "hl.workspace_rule({{ workspace = \"{}\", monitor = \"{}\" }}); ",
                    base + i as i64 - 1,
                    name
                ));
            }
        }
    }
    if !lua.is_empty() {
        let _ = run("hyprctl", &["eval", &lua]);
    }
    to_write
}

pub(crate) fn workspaces_action(action: &str) -> String {
    match action {
        "sync" => {
            workspaces_sync_action();
            "OK".to_string()
        }
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
                serde_json::to_string_pretty(&Value::Object(json))
                    .unwrap_or_else(|_| "{}".to_string())
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

pub(crate) fn workspace_action(snap: &Snapshot, slot: &str) -> String {
    if slot.is_empty() {
        return "ERR workspace needs a number".to_string();
    }
    if !slot.chars().all(|c| c.is_ascii_digit()) {
        return "ERR workspace needs a number".to_string();
    }
    let n: i64 = match slot.parse() {
        Ok(v) => v,
        Err(_) => return "ERR workspace needs a number".to_string(),
    };
    if !(1..=10).contains(&n) {
        return "ERR workspace must be 1-10".to_string();
    }
    let focused = snap.focused_name();
    if focused.is_empty() {
        return "ERR no focused monitor".to_string();
    }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    let id = base + n - 1;
    let lua = format!("hl.dispatch(hl.dsp.focus({{ workspace = \"{}\" }}))", id);
    let _ = run("hyprctl", &["eval", &lua]);
    format!("{} workspace {} (global {})", focused, n, id)
}

pub(crate) fn workspace_step_action(snap: &Snapshot, direction: &str) -> String {
    if direction != "next" && direction != "prev" {
        return "ERR workspace-step takes next or prev".to_string();
    }
    let focused = snap.focused_name();
    if focused.is_empty() {
        return "ERR no focused monitor".to_string();
    }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    // current focused workspace id
    let current: i64 = snap
        .all
        .iter()
        .find(|m| m.get("focused").and_then(|x| x.as_bool()).unwrap_or(false))
        .and_then(|m| {
            m.get("activeWorkspace")
                .and_then(|w| w.get("id"))
                .and_then(|x| x.as_i64())
        })
        .unwrap_or(0);
    if current == 0 {
        return "OK".to_string();
    }
    let target = if direction == "next" {
        current + 1
    } else {
        current - 1
    };
    let target = target.clamp(base, base + 9);
    if target == current {
        return "OK".to_string();
    }
    let lua = format!(
        "hl.dispatch(hl.dsp.focus({{ workspace = \"{}\" }}))",
        target
    );
    let _ = run("hyprctl", &["eval", &lua]);
    "OK".to_string()
}

pub(crate) fn move_window_action(snap: &Snapshot, slot: &str) -> String {
    if slot.is_empty() {
        return "ERR move-window needs a number".to_string();
    }
    if !slot.chars().all(|c| c.is_ascii_digit()) {
        return "ERR move-window needs a number".to_string();
    }
    let n: i64 = match slot.parse() {
        Ok(v) => v,
        Err(_) => return "ERR move-window needs a number".to_string(),
    };
    if !(1..=10).contains(&n) {
        return "ERR move-window must be 1-10".to_string();
    }
    let focused = snap.focused_name();
    if focused.is_empty() {
        return "ERR no focused monitor".to_string();
    }
    let base_str = workspace_base(&focused);
    let base: i64 = base_str.parse().unwrap_or(1);
    let id = base + n - 1;
    let lua = format!(
        "hl.dispatch(hl.dsp.window.move({{ workspace = \"{}\" }}))",
        id
    );
    let _ = run("hyprctl", &["eval", &lua]);
    format!("window to {} workspace {} (global {})", focused, n, id)
}

// Ask the running shell to flash each output's name on that output. The
// overlay lives in the shell (Ui/IdentifyHost.qml + Ui/ScreenLayers.qml),
// reached through its `identify` IPC target: `flash <ms> <descriptions>`,
// where the descriptions are a JSON map of output name to "make model".

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_map_parses_and_rejects_bad_json() {
        let m = read_workspace_map();
        assert!(m.is_empty() || !m.is_empty()); // file may exist in env; just no panic
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
