//! Power config: env overrides, tool paths, sysfs power state.
//! Prefixed names (ALEXIE_POWER_*, ANIXIE_POWER_*) preferred,
//! bare legacy names (PPD, LOW, AUTO...) fall back.

use std::env;

pub(crate) fn env_any(names: &[&str]) -> Option<String> {
    for n in names {
        if let Ok(v) = env::var(n) {
            return Some(v);
        }
    }
    None
}
pub(crate) fn env_str(names: &[&str], default: &str) -> String { env_any(names).unwrap_or_else(|| default.to_string()) }
pub(crate) fn ppd() -> String { env_str(&["ALEXIE_POWER_PPD", "ANIXIE_POWER_PPD", "PPD"], "powerprofilesctl") }
pub(crate) fn brightnessctl() -> String { env_str(&["ALEXIE_POWER_BRIGHTNESSCTL", "BRIGHTNESSCTL"], "brightnessctl") }
pub(crate) fn charge_limit_bin() -> String { env_str(&["ALEXIE_POWER_CHARGE_LIMIT", "CHARGE_LIMIT"], "charge-limit") }
pub(crate) fn sudo() -> String { env_str(&["ALEXIE_POWER_SUDO", "SUDO"], "sudo") }
pub(crate) fn ac_profile_default() -> String { env_any(&["ALEXIE_POWER_AC_PROFILE", "AC_PROFILE"]).unwrap_or_else(|| crate::config::get_str(&["power", "ac_profile"], "performance")) }
pub(crate) fn battery_profile_default() -> String { env_any(&["ALEXIE_POWER_BATTERY_PROFILE", "BATTERY_PROFILE"]).unwrap_or_else(|| crate::config::get_str(&["power", "battery_profile"], "power-saver")) }
pub(crate) fn low_profile() -> String { env_str(&["ALEXIE_POWER_LOW_PROFILE", "LOW_PROFILE"], "balanced") }
pub(crate) fn critical_profile() -> String { env_str(&["ALEXIE_POWER_CRITICAL_PROFILE", "CRITICAL_PROFILE"], "power-saver") }
pub(crate) fn low_pct() -> i64 { env_any(&["ALEXIE_POWER_LOW", "ANIXIE_POWER_LOW", "LOW"]).and_then(|s| s.parse().ok()).unwrap_or_else(|| crate::config::get_i64(&["power", "low"], 20)) }
pub(crate) fn critical_pct() -> i64 { env_any(&["ALEXIE_POWER_CRITICAL", "ANIXIE_POWER_CRITICAL", "CRITICAL"]).and_then(|s| s.parse().ok()).unwrap_or_else(|| crate::config::get_i64(&["power", "critical"], 8)) }
pub(crate) fn low_brightness() -> i64 { env_any(&["ALEXIE_POWER_LOW_BRIGHTNESS", "LOW_BRIGHTNESS"]).and_then(|s| s.parse().ok()).unwrap_or_else(|| crate::config::get_i64(&["power", "low_brightness"], 30)) }
pub(crate) fn critical_brightness() -> i64 { env_any(&["ALEXIE_POWER_CRITICAL_BRIGHTNESS", "CRITICAL_BRIGHTNESS"]).and_then(|s| s.parse().ok()).unwrap_or_else(|| crate::config::get_i64(&["power", "critical_brightness"], 10)) }
pub(crate) fn auto_default() -> bool { env_any(&["ALEXIE_POWER_AUTO", "ANIXIE_POWER_AUTO", "AUTO"]).map(|s| s == "1").unwrap_or(true) }


pub(crate) fn on_mains() -> bool {
    // check /sys/class/power_supply/*/online
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let p = e.path().join("online");
            if let Ok(s) = std::fs::read_to_string(&p)
                && s.trim() == "1" { return true; }
        }
    }
    false
}



pub(crate) fn battery_percent() -> Option<i64> {
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("BAT") { continue; }
            let p = e.path().join("capacity");
            if let Ok(s) = std::fs::read_to_string(&p)
                && let Ok(v) = s.trim().parse::<i64>() { return Some(v); }
        }
    }
    None
}

pub(crate) fn charge_ceiling() -> Option<i64> {
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let p = e.path().join("charge_control_end_threshold");
            if let Ok(s) = std::fs::read_to_string(&p)
                && let Ok(v) = s.trim().parse::<i64>() { return Some(v); }
        }
    }
    None
}


pub(crate) fn source_name() -> &'static str { if on_mains() { "ac" } else { "battery" } }
