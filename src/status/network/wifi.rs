//! Wi-Fi helpers: radio state, saved connections, line parsing.

use super::super::common::{nmcli, rfkill, run_out, split_nmcli_t_line};
use std::collections::HashMap;
pub(crate) fn is_wifi_enabled() -> bool {
    let out = run_out(&nmcli(), &["radio", "wifi"]);
    out.to_lowercase().contains("enabled")
}

pub(crate) fn is_airplane_mode() -> bool {
    let out = run_out(&rfkill(), &["-no", "TYPE,SOFT"]);
    let mut wlan_blocked = false;
    let mut bt_blocked = false;
    let mut has_wlan = false;
    let mut has_bt = false;
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let t = parts[0].to_lowercase();
            let blocked = parts[1].to_lowercase() == "blocked";
            if t.contains("wlan") || t.contains("wifi") {
                has_wlan = true;
                if blocked { wlan_blocked = true; }
            } else if t.contains("bluetooth") {
                has_bt = true;
                if blocked { bt_blocked = true; }
            }
        }
    }
    if has_wlan && has_bt {
        wlan_blocked && bt_blocked
    } else if has_wlan {
        wlan_blocked
    } else {
        false
    }
}

pub(crate) fn wifi_saved_connections() -> HashMap<String, bool> {
    let out = run_out(&nmcli(), &["-t", "-f", "NAME,TYPE,AUTOCONNECT", "connection", "show"]);
    let mut map = HashMap::new();
    for line in out.lines() {
        let parts = split_nmcli_t_line(line);
        if parts.len() >= 3 && parts[1] == "802-11-wireless" {
            map.insert(parts[0].clone(), parts[2].eq_ignore_ascii_case("yes"));
        }
    }
    map
}

/// Parse one wifi list line.
pub(crate) fn parse_wifi_line(line: &str) -> Option<(bool, String, String, i32, String, String)> {
    let parts = split_nmcli_t_line(line);
    if parts.len() < 6 {
        return None;
    }
    let active = parts[0].to_lowercase().contains("yes");
    let ssid = parts[1].trim().to_string();
    let bssid = parts[2].trim().to_string();
    let signal = parts[3].trim().parse::<i32>().unwrap_or(0);
    let rate = parts[4].trim().to_string();
    let security = parts[5].trim().to_string();
    Some((active, ssid, bssid, signal, rate, security))
}
