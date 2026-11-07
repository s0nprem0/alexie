//! Lid/internal/mirror helpers: clamshell state, lid file,
//! and the internal/mirror conveniences built on set.

use super::super::state::{self, Snapshot};
use super::super::workspaces::workspaces_sync_action;

pub(crate) fn lid_closed() -> bool {
    // read /proc/acpi/button/lid/*/state
    if let Ok(entries) = std::fs::read_dir("/proc/acpi/button/lid") {
        for e in entries.flatten() {
            let p = e.path().join("state");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if s.contains("closed") {
                    return true;
                }
                if s.contains("open") {
                    return false;
                }
            }
        }
    }
    // fallback: /sys/class/power_supply
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let p = e.path().join("lid/state");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if s.contains("closed") {
                    return true;
                }
                if s.contains("open") {
                    return false;
                }
            }
        }
    }
    false
}

pub(crate) fn clear_disabled(output: &str) {
    let path = state::state_file();
    if !path.exists() {
        return;
    }
    if let Ok(content) = std::fs::read_to_string(&path) {
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let mut changed = false;
        for line in lines.iter_mut() {
            if line.starts_with('#') {
                continue;
            }
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.len() >= 4 && t[0] == output && t[3] == "1" {
                // set 4th field (index 3) to 0
                let mut f: Vec<String> = t.iter().map(|s| s.to_string()).collect();
                while f.len() < 9 {
                    f.push(String::new());
                }
                f[3] = "0".to_string();
                *line = f.join(" ");
                changed = true;
            }
        }
        if changed {
            if lines.is_empty() || !lines[0].starts_with("# v2") {
                lines.insert(0, "# v2".to_string());
            }
            let _ = state::atomic_write(&path, &(lines.join("\n") + "\n"));
        }
    }
}

pub(crate) fn internal_action(snap: &Snapshot, action: &str) -> String {
    let name = snap.internal_name();
    if name.is_empty() {
        return "ERR no monitor found".to_string();
    }
    let disabled = snap.field(&name, "disabled");
    let action = if action == "toggle" {
        if disabled == "true" { "on" } else { "off" }
    } else {
        action
    };
    match action {
        "off" => {
            if disabled == "true" {
                return "OK".to_string();
            }
            if snap.active_count() <= 1 {
                return "ERR refusing to disable the only active output".to_string();
            }
            if let Err(e) = state::apply_change_snap(snap, &name, "disabled", "true") {
                return format!("ERR {}", e);
            }
            if let Err(e) = state::save_field_snap(snap, &name, "disabled", "1") {
                return format!("ERR {}", e);
            }
            format!("internal panel {}: off", name)
        }
        "on" => {
            if disabled != "true" {
                return "OK".to_string();
            }
            if let Err(e) = state::apply_change_snap(snap, &name, "disabled", "false") {
                return format!("ERR {}", e);
            }
            if let Err(e) = state::save_field_snap(snap, &name, "disabled", "0") {
                return format!("ERR {}", e);
            }
            let _ = workspaces_sync_action();
            format!("internal panel {}: on", name)
        }
        _ => "ERR internal takes on, off or toggle".to_string(),
    }
}

pub(crate) fn mirror_action(snap: &Snapshot, action: &str) -> String {
    let internal = snap.internal_name();
    let external = snap.external_name();
    if external.is_empty() {
        return "ERR no external monitor connected to mirror onto".to_string();
    }
    if snap.field(&internal, "disabled") == "true" {
        return "ERR internal panel is off; turn it on before mirroring".to_string();
    }
    let mirrored = snap.field(&external, "mirrorOf");
    let mirrored = if mirrored.is_empty() || mirrored == "null" {
        "none".to_string()
    } else {
        mirrored
    };
    let action = if action == "toggle" {
        if mirrored == "none" { "on" } else { "off" }
    } else {
        action
    };
    match action {
        "on" => {
            if let Err(e) = state::apply_change_snap(snap, &external, "mirror", &internal) {
                return format!("ERR {}", e);
            }
            if let Err(e) = state::save_field_snap(snap, &external, "mirror", &internal) {
                return format!("ERR {}", e);
            }
            format!("{} now mirrors {}", external, internal)
        }
        "off" => {
            if let Err(e) = state::apply_change_snap(snap, &external, "mirror", "none") {
                return format!("ERR {}", e);
            }
            if let Err(e) = state::save_field_snap(snap, &external, "mirror", "none") {
                return format!("ERR {}", e);
            }
            let _ = workspaces_sync_action();
            format!("{} extended again", external)
        }
        _ => "ERR mirror takes on, off or toggle".to_string(),
    }
}

pub(crate) fn lid_action(snap: &Snapshot, action: &str) -> String {
    let internal = snap.internal_name();
    if internal.is_empty() {
        return "OK".to_string();
    }
    let external = snap.external_name();
    match action {
        "close" | "off" => {
            if external.is_empty() {
                return "no external monitor; leaving the internal panel on".to_string();
            }
            if snap.field(&internal, "disabled") == "true" {
                return "OK".to_string();
            }
            if let Err(e) = state::apply_change_snap(snap, &internal, "disabled", "true") {
                return format!("ERR {}", e);
            }
            if let Err(e) = state::save_field_snap(snap, &internal, "disabled", "1") {
                return format!("ERR {}", e);
            }
            let _ = std::fs::create_dir_all(state::state_dir());
            let _ = std::fs::write(state::lid_file(), format!("{}\n", internal));
            format!("lid closed: internal panel {} off", internal)
        }
        "open" | "on" => {
            let saved = match std::fs::read_to_string(state::lid_file()) {
                Ok(s) => s.trim().to_string(),
                Err(_) => String::new(),
            };
            if saved.is_empty() {
                return "OK".to_string();
            }
            if snap.field(&saved, "name").is_empty() {
                let _ = std::fs::remove_file(state::lid_file());
                return "OK".to_string();
            }
            if let Err(e) = state::apply_change_snap(snap, &saved, "disabled", "false") {
                return format!("ERR {}", e);
            }
            if let Err(e) = state::save_field_snap(snap, &saved, "disabled", "0") {
                return format!("ERR {}", e);
            }
            let _ = std::fs::remove_file(state::lid_file());
            let _ = workspaces_sync_action();
            format!("lid opened: internal panel {} on", saved)
        }
        "toggle" => {
            if state::lid_file().exists() {
                lid_action(snap, "open")
            } else {
                lid_action(snap, "close")
            }
        }
        "reconcile" => {
            if lid_closed() && !external.is_empty() {
                lid_action(snap, "close")
            } else {
                lid_action(snap, "open")
            }
        }
        _ => "ERR lid takes close, open, toggle or reconcile".to_string(),
    }
}
