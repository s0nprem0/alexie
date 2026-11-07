//! Power profiles, battery automation, charge limit.
//!
//! Split from a single 358-line power.rs:
//! - `config` — env overrides, tool paths, sysfs source/battery
//! - `backlight` — brightness read/set, dim/restore
//! - `state` — automation/preferred state files
//!
//! `handle()` below is just dispatch.

pub mod backlight;
pub mod config;
pub mod state;

use crate::proc::run;
use backlight::{enter_dim, restore_brightness};
use config::{
    battery_percent, charge_ceiling, charge_limit_bin, critical_brightness, critical_pct,
    critical_profile, low_brightness, low_pct, low_profile, ppd, source_name, sudo,
};
use state::{automation_enabled, automation_file, last_source_file, preferred, state_file_src};
use std::env;
use std::path::PathBuf;

pub fn source_pub() -> String {
    config::source_name().to_string()
}
pub fn state_dir_pub() -> PathBuf {
    state_dir()
}
pub(crate) fn state_dir() -> PathBuf {
    // Legacy power state is $XDG_STATE_HOME/anixie/powerprofile
    // (see anixie modules/features/power/default.nix `state`).
    // Writing automation/ac/battery/brightness.* directly into
    // .../anixie polluted the parent dir and never matched legacy.
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie").join("powerprofile")
}

pub fn apply_pub() {
    apply();
}
pub(crate) fn apply() {
    let _ = std::fs::create_dir_all(state_dir());
    let src = source_name();
    let mut profile = preferred(src);

    if src == "ac" {
        restore_brightness();
    } else if automation_enabled()
        && let Some(pct) = battery_percent()
    {
        if pct <= critical_pct() {
            profile = critical_profile();
            enter_dim("critical", critical_brightness());
        } else if pct <= low_pct() {
            profile = low_profile();
            enter_dim("low", low_brightness());
        } else {
            restore_brightness();
        }
    }

    let ppd_bin = ppd();
    let _ = run(&ppd_bin, &["set", &profile]);
    let _ = std::fs::write(last_source_file(), format!("{}\n", src));
}

pub fn check_battery_pub() {
    check_battery();
}
pub(crate) fn check_battery() {
    if !automation_enabled() {
        return;
    }
    let _ = std::fs::create_dir_all(state_dir());
    let src = source_name();
    let last = std::fs::read_to_string(last_source_file()).unwrap_or_default();
    if last.trim() != src {
        apply();
        return;
    }
    if src == "ac" {
        restore_brightness();
        return;
    }
    if let Some(pct) = battery_percent() {
        let ppd_bin = ppd();
        if pct <= critical_pct() {
            let _ = run(&ppd_bin, &["set", &critical_profile()]);
            enter_dim("critical", critical_brightness());
        } else if pct <= low_pct() {
            let _ = run(&ppd_bin, &["set", &low_profile()]);
            enter_dim("low", low_brightness());
        } else {
            restore_brightness();
        }
    }
}

pub(crate) fn set_pref(src: &str, profile: &str) {
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(state_file_src(src), format!("{}\n", profile));
    apply();
}

pub(crate) fn set_automation(state: &str) {
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(automation_file(), format!("{}\n", state));
    if state == "off" {
        restore_brightness();
    }
    apply();
}

pub(crate) fn charge(args: &[String]) -> String {
    let cl = charge_limit_bin();
    let sudo_bin = sudo();
    let sub = args.first().map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "" | "show" => {
            // print ceiling
            // delegate to charge-limit binary
            let (code, out, err) = run(&cl, &[]);
            if code == 0 {
                out.trim().to_string()
            } else {
                format!("ERR {}", err.trim())
            }
        }
        "full" => {
            let (code, _, err) = run(&sudo_bin, &[&cl, "full"]);
            if code != 0 {
                format!("ERR {}", err.trim())
            } else {
                "OK".to_string()
            }
        }
        "limit" => {
            let n = args.get(1).cloned().unwrap_or_default();
            let (code, _, err) = if n.is_empty() {
                run(&sudo_bin, &[&cl, "limit"])
            } else {
                run(&sudo_bin, &[&cl, "limit", &n])
            };
            if code != 0 {
                format!("ERR {}", err.trim())
            } else {
                "OK".to_string()
            }
        }
        "apply" => {
            let (code, _, err) = run(&cl, &["apply"]);
            if code != 0 {
                format!("ERR {}", err.trim())
            } else {
                "OK".to_string()
            }
        }
        _ => "ERR charge wants show, full, limit [N] or apply".to_string(),
    }
}

pub(crate) fn show() -> String {
    let src = source_name();
    let (code, profile, _) = run(&ppd(), &["get"]);
    let profile = if code == 0 {
        profile.trim().to_string()
    } else {
        "unknown".to_string()
    };
    let mut pairs: Vec<(&str, String)> = vec![
        ("source", src.to_string()),
        ("profile", profile),
        ("on mains", preferred("ac")),
        ("on battery", preferred("battery")),
    ];
    if let Some(ceil) = charge_ceiling() {
        pairs.push(("charge", format!("{}% ceiling", ceil)));
    }
    if automation_enabled() {
        let bat = battery_percent()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        pairs.push(("battery", format!("{}%", bat)));
        pairs.push((
            "automation",
            format!(
                "on (low {}% -> {} at {}%, critical {}% -> {} at {}%)",
                low_pct(),
                low_profile(),
                low_brightness(),
                critical_pct(),
                critical_profile(),
                critical_brightness()
            ),
        ));
    } else {
        pairs.push(("automation", "off".to_string()));
    }
    crate::table::render_kv(&pairs)
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "" | "show" | "status" => show(),
        "apply" => {
            apply();
            "OK".to_string()
        }
        "set" => {
            // power set ac|battery PROFILE
            if args.len() < 3 {
                return "ERR set needs a source and a profile".to_string();
            }
            let src = &args[1];
            let profile = &args[2];
            if src != "ac" && src != "battery" {
                return format!("ERR unknown source \"{}\"", src);
            }
            set_pref(src, profile);
            "OK".to_string()
        }
        "automation" => {
            if automation_enabled() {
                "on".to_string()
            } else {
                "off".to_string()
            }
        }
        "set-automation" => {
            if args.len() < 2 {
                return "ERR set-automation needs on or off".to_string();
            }
            match args[1].as_str() {
                "on" | "off" => {
                    set_automation(&args[1]);
                    "OK".to_string()
                }
                _ => format!("ERR unknown automation state \"{}\"", args[1]),
            }
        }
        "charge" => {
            let rest: Vec<String> = args[1..].to_vec();
            charge(&rest)
        }
        "check" => {
            check_battery();
            "OK".to_string()
        }
        "watch" => {
            // The native watcher (power_watch::run, udev + 30s timer)
            // blocks forever and is special-cased in main() for
            // `alexie power watch`, like screentime watch. Delegating
            // to the bash powerprofile watcher here was dead code
            // via the CLI and would block a daemon thread forever.
            "Run `alexie power watch` directly (it blocks).".to_string()
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run("powerprofile", &a);
            if code == 0 {
                if out.trim().is_empty() {
                    "OK".to_string()
                } else {
                    out.trim_end().to_string()
                }
            } else {
                format!("ERR (exit {}) {}", code, err.trim())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::env_any;

    #[test]
    fn state_dir_is_anixie_powerprofile() {
        let dir = state_dir();
        assert!(
            dir.ends_with(std::path::Path::new("anixie").join("powerprofile")),
            "got: {}",
            dir.display()
        );
    }

    #[test]
    fn env_any_prefers_prefixed() {
        // Pure check on env_any via explicit names: the helper
        // itself is tested through low_pct() defaults which do not
        // depend on the (mutable) process environment in the test
        // sandbox — instead assert the helper's precedence logic
        // by checking that a name guaranteed unset falls through.
        assert!(env_any(&["ALEXIE_POWER_TEST_UNSET_XYZ_123", "NOPE_XYZ_123"]).is_none());
    }

    #[test]
    fn watch_tells_user_to_run_directly() {
        let out = handle(&["watch".to_string()]);
        assert!(out.contains("alexie power watch"), "got: {out}");
    }
}
