//! Network status (wabi `get_network_status` JSON).
//!
//! Split from a single 503-line `network.rs`:
//! - `types` — WifiNetwork/VpnConnection/ConnectionDetails/NetworkStatus
//! - `wifi` — radio state, saved connections, line parsing
//! - `collect` — wifi networks, VPNs, details, warp/tailscale
//!
//! `to_json()`/`render_text()`/`handle()` below just orchestrate.

pub(crate) mod types;
pub(crate) mod wifi;
pub(crate) mod collect;

#[allow(unused_imports)] // re-exported for public API compat
pub use types::{ConnectionDetails, NetworkStatus, VpnConnection, WifiNetwork};

use super::common::print_json;
pub(crate) use collect::collect;
#[cfg(test)]
use wifi::parse_wifi_line;

pub(crate) fn to_json(status: &NetworkStatus) -> String {
    print_json(status)
}

pub(crate) fn render_text(s: &NetworkStatus) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "wifi {}  airplane {}  connected {}  ssid {}  signal {}%  speed {}\n",
        if s.wifi_enabled { "on" } else { "off" },
        if s.airplane_mode { "on" } else { "off" },
        if s.connected { "yes" } else { "no" },
        if s.active_ssid.is_empty() { "-" } else { &s.active_ssid },
        s.active_signal,
        if s.active_speed.is_empty() { "-" } else { &s.active_speed },
    ));
    if !s.details.ip_address.is_empty() {
        out.push_str(&format!(
            "ip {}  gw {}  dns {}  subnet {}\n",
            s.details.ip_address,
            if s.details.gateway.is_empty() { "-" } else { &s.details.gateway },
            if s.details.dns.is_empty() { "-" } else { &s.details.dns },
            if s.details.subnet.is_empty() { "-" } else { &s.details.subnet },
        ));
    }
    if !s.vpns.is_empty() {
        let rows: Vec<Vec<String>> = s
            .vpns
            .iter()
            .map(|v| vec![v.name.clone(), v.vpn_type.clone(), v.device.clone(), if v.active { "yes".into() } else { "no".into() }])
            .collect();
        out.push_str(&crate::table::render(&["VPN", "Type", "Device", "Active"], &rows));
        out.push('\n');
    }
    if !s.networks.is_empty() {
        let rows: Vec<Vec<String>> = s
            .networks
            .iter()
            .map(|n| {
                vec![
                    n.ssid.clone(),
                    format!("{}%", n.signal),
                    n.rate.clone(),
                    n.security.clone(),
                    if n.active { "yes".into() } else { "no".into() },
                    if n.saved { "yes".into() } else { "no".into() },
                ]
            })
            .collect();
        out.push_str(&crate::table::render(&["SSID", "Signal", "Rate", "Security", "Active", "Saved"], &rows));
    }
    if out.trim().is_empty() {
        out.push_str("network: no data (nmcli unavailable or wifi off)");
    }
    out.trim_end().to_string()
}

pub(crate) fn handle(args: &[String]) -> String {
    // `network [--json] [--rescan]`
    let want_json = args.iter().any(|a| a == "--json");
    let force_rescan = args.iter().any(|a| a == "--rescan");
    // Reject unknown flags (except bare --json/--rescan).
    for a in args {
        if a.starts_with("--") && a != "--json" && a != "--rescan" {
            return format!("ERR unknown flag: {}", a);
        }
    }
    let status = collect(force_rescan);
    if want_json {
        to_json(&status)
    } else {
        render_text(&status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_line_parses() {
        let line = "yes:My\\:Wifi:AA\\:BB\\:CC:80:540 Mbit/s:WPA2";
        let (active, ssid, bssid, signal, rate, security) = parse_wifi_line(line).unwrap();
        assert!(active);
        assert_eq!(ssid, "My:Wifi");
        assert_eq!(bssid, "AA:BB:CC");
        assert_eq!(signal, 80);
        assert_eq!(rate, "540 Mbit/s");
        assert_eq!(security, "WPA2");
    }

    #[test]
    fn handle_unknown_flag_fails_fast() {
        let out = handle(&["--bogus".to_string()]);
        assert!(out.starts_with("ERR unknown flag:"), "got: {out}");
    }

    #[test]
    fn handle_json_is_parseable() {
        let out = handle(&["--json".to_string()]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("network --json must be JSON");
        assert!(v.get("wifi_enabled").is_some());
        assert!(v.get("networks").is_some());
    }

    #[test]
    fn render_text_contains_ssid_or_off() {
        let s = NetworkStatus {
            wifi_enabled: false,
            airplane_mode: true,
            connected: false,
            active_ssid: String::new(),
            active_signal: 0,
            active_speed: String::new(),
            wifi_device: String::new(),
            warp_connected: false,
            tailscale_connected: false,
            tailscale_ip: String::new(),
            details: ConnectionDetails::default(),
            networks: Vec::new(),
            vpns: Vec::new(),
        };
        let t = render_text(&s);
        assert!(t.contains("wifi off"));
    }
}
