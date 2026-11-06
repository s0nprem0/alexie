use super::pending::{read_pending, remove_pending, write_pending};
use super::state::{self, Snapshot};
use super::validate::validate_setting;
use super::workspaces::workspaces_sync_action;
use crate::proc::run;

pub(crate) fn old_value_for(snap: &Snapshot, output: &str, key: &str) -> Option<String> {
    let st = state::read_state_from(snap, output)?;
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

pub(crate) fn set_action(snap: &Snapshot, output: &str, key: &str, value: &str) -> String {
    // refuse unknown output
    if snap.field(output, "name").is_empty() {
        return format!("ERR no output called \"{}\"", output);
    }

    match key {
        "scale" => {
            let normalized = match validate_setting("scale", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = state::apply_change_snap(snap, output, "scale", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "scale", &normalized) { return format!("ERR {}", e); }
            format!("{} scale {}", output, normalized)
        }
        "transform" => {
            let normalized = match validate_setting("transform", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = state::apply_change_snap(snap, output, "transform", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "transform", &normalized) { return format!("ERR {}", e); }
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
            if let Err(e) = state::apply_change_snap(snap, output, "disabled", disabled) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "disabled", &index.to_string()) { return format!("ERR {}", e); }
            if index == 0 {
                let _ = workspaces_sync_action();
            }
            format!("{} disabled {}", output, value)
        }
        "position" => {
            let normalized = match validate_setting("position", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = state::apply_change_snap(snap, output, "position", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "position", &normalized) { return format!("ERR {}", e); }
            format!("{} position {}", output, normalized)
        }
        "mode" => {
            let normalized = match validate_setting("mode", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = state::apply_change_snap(snap, output, "mode", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "mode", &normalized) { return format!("ERR {}", e); }
            format!("{} mode {}", output, normalized)
        }
        "sdrbrightness" => {
            let normalized = match validate_setting("sdrbrightness", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = state::apply_change_snap(snap, output, "sdrbrightness", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "sdrbrightness", &normalized) { return format!("ERR {}", e); }
            format!("{} sdrbrightness {}", output, normalized)
        }
        "sdrsaturation" => {
            let normalized = match validate_setting("sdrsaturation", value) { Ok(v) => v, Err(e) => return e };
            if let Err(e) = state::apply_change_snap(snap, output, "sdrsaturation", &normalized) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "sdrsaturation", &normalized) { return format!("ERR {}", e); }
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
                let st = state::read_state_from(snap, &v);
                if let Some(s) = st {
                    if s.3 != "none" { return format!("ERR {} is mirroring another screen; extend it first", v); }
                }
            }
            if snap.field(output, "disabled") == "true" {
                return format!("ERR {} is off; turn it on before mirroring", output);
            }
            if let Err(e) = state::apply_change_snap(snap, output, "mirror", &v) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, output, "mirror", &v) { return format!("ERR {}", e); }
            if v == "none" {
                let _ = workspaces_sync_action();
            }
            if v == "none" { format!("{} extended again", output) } else { format!("{} now mirrors {}", output, v) }
        }
        _ => "ERR set takes scale, transform, disabled, position, mode, mirror, sdrbrightness or sdrsaturation".to_string(),
    }
}

pub(crate) fn try_action(snap: &Snapshot, args: &[String]) -> String {
    if args.len() < 3 { return "ERR try needs an output, a setting and a value".to_string(); }
    let output = &args[0]; let key = &args[1]; let raw_value = &args[2];
    if output.is_empty() || !output.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return format!("ERR invalid output name: {}", output);
    }
    let pending = state::state_dir().join("display-pending");
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

    let _ = std::fs::create_dir_all(state::state_dir());
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

    if let Err(e) = state::apply_change_snap(snap, output, key, &value) { return format!("ERR {}", e); }

    format!("{} {} {} (reverting in 15s unless kept)", output, key, value)
}


pub(crate) fn lid_closed() -> bool {
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

pub(crate) fn clear_disabled(output: &str) {
    let path = state::state_file();
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
            let _ = state::atomic_write(&path, &(lines.join("\n") + "\n"));
        }
    }
}

pub(crate) fn internal_action(snap: &Snapshot, action: &str) -> String {
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
            if let Err(e) = state::apply_change_snap(snap, &name, "disabled", "true") { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, &name, "disabled", "1") { return format!("ERR {}", e); }
            format!("internal panel {}: off", name)
        }
        "on" => {
            if disabled != "true" { return "OK".to_string(); }
            if let Err(e) = state::apply_change_snap(snap, &name, "disabled", "false") { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, &name, "disabled", "0") { return format!("ERR {}", e); }
            let _ = workspaces_sync_action();
            format!("internal panel {}: on", name)
        }
        _ => "ERR internal takes on, off or toggle".to_string(),
    }
}

pub(crate) fn mirror_action(snap: &Snapshot, action: &str) -> String {
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
            if let Err(e) = state::apply_change_snap(snap, &external, "mirror", &internal) { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, &external, "mirror", &internal) { return format!("ERR {}", e); }
            format!("{} now mirrors {}", external, internal)
        }
        "off" => {
            if let Err(e) = state::apply_change_snap(snap, &external, "mirror", "none") { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, &external, "mirror", "none") { return format!("ERR {}", e); }
            let _ = workspaces_sync_action();
            format!("{} extended again", external)
        }
        _ => "ERR mirror takes on, off or toggle".to_string(),
    }
}

pub(crate) fn lid_action(snap: &Snapshot, action: &str) -> String {
    let internal = snap.internal_name();
    if internal.is_empty() { return "OK".to_string(); }
    let external = snap.external_name();
    match action {
        "close" | "off" => {
            if external.is_empty() {
                return "no external monitor; leaving the internal panel on".to_string();
            }
            if snap.field(&internal, "disabled") == "true" { return "OK".to_string(); }
            if let Err(e) = state::apply_change_snap(snap, &internal, "disabled", "true") { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, &internal, "disabled", "1") { return format!("ERR {}", e); }
            let _ = std::fs::create_dir_all(state::state_dir());
            let _ = std::fs::write(state::lid_file(), format!("{}\n", internal));
            format!("lid closed: internal panel {} off", internal)
        }
        "open" | "on" => {
            let saved = match std::fs::read_to_string(state::lid_file()) { Ok(s) => s.trim().to_string(), Err(_) => String::new() };
            if saved.is_empty() { return "OK".to_string(); }
            if snap.field(&saved, "name").is_empty() { let _ = std::fs::remove_file(state::lid_file()); return "OK".to_string(); }
            if let Err(e) = state::apply_change_snap(snap, &saved, "disabled", "false") { return format!("ERR {}", e); }
            if let Err(e) = state::save_field_snap(snap, &saved, "disabled", "0") { return format!("ERR {}", e); }
            let _ = std::fs::remove_file(state::lid_file());
            let _ = workspaces_sync_action();
            format!("lid opened: internal panel {} on", saved)
        }
        "toggle" => {
            if state::lid_file().exists() { lid_action(snap, "open") } else { lid_action(snap, "close") }
        }
        "reconcile" => {
            if lid_closed() && !external.is_empty() { lid_action(snap, "close") } else { lid_action(snap, "open") }
        }
        _ => "ERR lid takes close, open, toggle or reconcile".to_string(),
    }
}

pub(crate) fn recover_action(snap: &Snapshot) -> String {
    // clear stale lid state if lid is open
    if state::lid_file().exists() && !lid_closed() {
        if let Ok(saved) = std::fs::read_to_string(state::lid_file()) {
            let saved = saved.trim().to_string();
            if !saved.is_empty() && !snap.field(&saved, "name").is_empty() {
                clear_disabled(&saved);
            }
        }
        let _ = std::fs::remove_file(state::lid_file());
        let _ = workspaces_sync_action();
        return "lid is open; cleared the saved clamshell state".to_string();
    }
    "OK".to_string()
}
pub(crate) fn keep_action(snap: &Snapshot) -> String {
    let pending = state::state_dir().join("display-pending");
    if !pending.exists() { return "no display change to keep".to_string(); }
    let Some(p) = read_pending() else {
        let _ = std::fs::remove_file(&pending);
        return "ERR could not read pending".to_string();
    };

    let _ = run("systemctl", &["--user", "stop", "alexie-display-revert.timer"]);
    if let Err(e) = state::save_field_snap(snap, &p.output, &p.key, &p.new_value) { return format!("ERR {}", e); }
    remove_pending(&pending);
    format!("{} {} {} kept", p.output, p.key, p.new_value)
}

pub(crate) fn revert_action(snap: &Snapshot) -> String {
    let pending = state::state_dir().join("display-pending");
    if !pending.exists() { return "OK".to_string(); }
    let Some(p) = read_pending() else {
        return "OK".to_string();
    };

    if snap.field(&p.output, "name").is_empty() {
        remove_pending(&pending);
        return "OK".to_string();
    }

    let _ = run("systemctl", &["--user", "stop", "alexie-display-revert.timer"]);
    if let Err(e) = state::apply_change_snap(snap, &p.output, &p.key, &p.old_value) { remove_pending(&pending); return format!("ERR {}", e); }
    remove_pending(&pending);
    format!("{} {} reverted to {}", p.output, p.key, p.old_value)
}

