//! Recover/keep/revert: pending-change finalisation, plus
//! lid-state recovery when the lid is open.

use super::super::pending::{read_pending, remove_pending};
use super::super::state::{self, Snapshot};
use super::super::workspaces::workspaces_sync_action;
use super::lid::{clear_disabled, lid_closed};
use crate::proc::run;

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
    if !pending.exists() {
        return "no display change to keep".to_string();
    }
    let Some(p) = read_pending() else {
        let _ = std::fs::remove_file(&pending);
        return "ERR could not read pending".to_string();
    };

    let _ = run(
        "systemctl",
        &["--user", "stop", "alexie-display-revert.timer"],
    );
    if let Err(e) = state::save_field_snap(snap, &p.output, &p.key, &p.new_value) {
        return format!("ERR {}", e);
    }
    remove_pending(&pending);
    format!("{} {} {} kept", p.output, p.key, p.new_value)
}

pub(crate) fn revert_action(snap: &Snapshot) -> String {
    let pending = state::state_dir().join("display-pending");
    if !pending.exists() {
        return "OK".to_string();
    }
    let Some(p) = read_pending() else {
        return "OK".to_string();
    };

    if snap.field(&p.output, "name").is_empty() {
        remove_pending(&pending);
        return "OK".to_string();
    }

    let _ = run(
        "systemctl",
        &["--user", "stop", "alexie-display-revert.timer"],
    );
    if let Err(e) = state::apply_change_snap(snap, &p.output, &p.key, &p.old_value) {
        remove_pending(&pending);
        return format!("ERR {}", e);
    }
    remove_pending(&pending);
    format!("{} {} reverted to {}", p.output, p.key, p.old_value)
}
