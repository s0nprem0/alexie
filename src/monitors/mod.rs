//! Monitors: display scaling, mode, position, mirroring, workspaces.
//!
//! Split from a single 1470-line monitors.rs for readability:
//! - `state` — Snapshot, state file IO, hyprctl fetching
//! - `validate` — setting validation/normalisation
//! - `pending` — try/keep/revert pending-change file + lock
//! - `workspaces` — workspace map, sync, DDC discovery
//! - `identify` — identify overlay via the shell IPC
//! - `actions` — set/try/internal/mirror/lid/keep/revert actions
//!
//! `handle()` below is just the dispatcher over those.

pub mod state;
pub mod list;
pub mod validate;
pub mod pending;
pub mod workspaces;
pub mod identify;
pub mod actions;

use crate::proc::run;
use state::Snapshot;

pub fn focused_name() -> String {
    state::focused_name()
}

pub fn handle(args: &[String]) -> String {
    // Bare `alexie monitors` shows status — the read-only default,
    // matching `power`/`screentime`/`toggle` — instead of erroring.
    let default_args;
    let args = if args.is_empty() {
        default_args = vec!["status".to_string()];
        default_args.as_slice()
    } else {
        args
    };
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
        "list" => list::list_action(&snap),
        "set" => {
            if args.len() < 4 {
                return "ERR usage: monitors set OUTPUT KEY VALUE".to_string();
            }
            actions::set_action(&snap, &args[1], &args[2], &args[3])
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
                    if let Err(e) = state::apply_change_snap(&snap, &focused, "scale", v) { return format!("ERR {}", e); }
                    if let Err(e) = state::save_field_snap(&snap, &focused, "scale", v) { return format!("ERR {}", e); }
                    format!("{} scale set to {}", focused, v)
                }
                "reset" => {
                    if let Err(e) = state::apply_change_snap(&snap, &focused, "scale", "1") { return format!("ERR {}", e); }
                    if let Err(e) = state::save_field_snap(&snap, &focused, "scale", "1") { return format!("ERR {}", e); }
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
                    if let Err(e) = state::apply_change_snap(&snap, &focused, "scale", newv) { return format!("ERR {}", e); }
                    if let Err(e) = state::save_field_snap(&snap, &focused, "scale", newv) { return format!("ERR {}", e); }
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
                if is_internal { vec![] } else if let Some(d) = workspaces::ddcutil_display(&focused) {
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
        "identify" => identify::identify_action(&snap, args),
        "wake" => {
            let _ = run("hyprctl", &["dispatch", "dpms", "on"]);
            "OK".to_string()
        }
        "recover" => actions::recover_action(&snap),
        "internal" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            actions::internal_action(&snap, action)
        }
        "mirror" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            actions::mirror_action(&snap, action)
        }
        "lid" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "toggle" };
            actions::lid_action(&snap, action)
        }
        "try" => {
            if args.len() < 4 { return "ERR try needs an output, a setting and a value".to_string(); }
            actions::try_action(&snap, &args[1..])
        }
        "keep" => actions::keep_action(&snap),
        "revert" => actions::revert_action(&snap),
        "workspaces" => {
            let action = if args.len() >= 2 { args[1].as_str() } else { "sync" };
            workspaces::workspaces_action(action)
        }
        "workspace" => {
            let slot = if args.len() >= 2 { args[1].as_str() } else { "" };
            workspaces::workspace_action(&snap, slot)
        }
        "workspace-step" => {
            let dir = if args.len() >= 2 { args[1].as_str() } else { "next" };
            workspaces::workspace_step_action(&snap, dir)
        }
        "move-window" => {
            let slot = if args.len() >= 2 { args[1].as_str() } else { "" };
            workspaces::move_window_action(&snap, slot)
        }
        "identities" => identify::identities_action(&snap),
        "sync-identities" => identify::sync_identities_action(&snap),
        _ => format!("ERR unknown monitors subcommand: {}", args[0]),
    }
}

