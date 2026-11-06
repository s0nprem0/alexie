//! Set/try: validation, apply, save. Mirrors the legacy
//! `monitors set/try` arms.

use super::super::pending::write_pending;
use super::super::state::{self, Snapshot};
use super::super::validate::validate_setting;
use super::super::workspaces::workspaces_sync_action;
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


