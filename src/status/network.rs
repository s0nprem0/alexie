//! Network status (wabi `get_network_status` JSON).

use super::common::{active_wifi_device, cidr_to_netmask, print_json, run_out, split_nmcli_t_line, cache_dir, nmcli, rfkill, iw, warp_cli, tailscale};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Clone, Debug)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: String,
    pub signal: i32,
    pub rate: String,
    pub security: String,
    pub active: bool,
    pub saved: bool,
    pub autoconnect: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct VpnConnection {
    pub name: String,
    pub vpn_type: String,
    pub device: String,
    pub active: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ConnectionDetails {
    pub ip_address: String,
    pub gateway: String,
    pub dns: String,
    pub subnet: String,
    pub security: String,
    pub bssid: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct NetworkStatus {
    pub wifi_enabled: bool,
    pub airplane_mode: bool,
    pub connected: bool,
    pub active_ssid: String,
    pub active_signal: i32,
    pub active_speed: String,
    pub wifi_device: String,
    pub warp_connected: bool,
    pub tailscale_connected: bool,
    pub tailscale_ip: String,
    pub details: ConnectionDetails,
    pub networks: Vec<WifiNetwork>,
    pub vpns: Vec<VpnConnection>,
}

fn is_wifi_enabled() -> bool {
    let out = run_out(&nmcli(), &["radio", "wifi"]);
    out.to_lowercase().contains("enabled")
}

fn is_airplane_mode() -> bool {
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

fn wifi_saved_connections() -> HashMap<String, bool> {
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

pub(crate) fn collect(force_rescan: bool) -> NetworkStatus {
    let wifi_enabled = is_wifi_enabled();
    let airplane_mode = is_airplane_mode();
    let wifi_device = active_wifi_device();

    let mut connected = false;
    let mut active_ssid = String::new();
    let mut active_signal = 0;
    let mut active_speed = String::new();
    let mut details = ConnectionDetails::default();
    let mut networks: Vec<WifiNetwork> = Vec::new();
    let mut vpns: Vec<VpnConnection> = Vec::new();
    let saved_connections = wifi_saved_connections();

    if wifi_enabled {
        let rescan_val = if force_rescan { "yes" } else { "no" };
        let wifi_list_out = run_out(
            &nmcli(),
            &[
                "-t",
                "-f",
                "active,ssid,bssid,signal,rate,security",
                "dev",
                "wifi",
                "list",
                "--rescan",
                rescan_val,
            ],
        );

        let mut seen_ssids = HashSet::new();

        for line in wifi_list_out.lines() {
            let Some((active, ssid, bssid, signal, rate, security)) = parse_wifi_line(line) else {
                continue;
            };
            if ssid.is_empty() {
                continue;
            }

            if active {
                connected = true;
                active_ssid = ssid.clone();
                active_signal = signal;
                details.security = security.clone();
                details.bssid = bssid.clone();

                let mut tx_rate = String::new();
                let mut rx_rate = String::new();
                let link_dev = wifi_device.as_deref().unwrap_or("wlo1").to_string();
                let link_info = run_out(&iw(), &["dev", &link_dev, "link"]);
                for l in link_info.lines() {
                    let trimmed = l.trim();
                    if trimmed.starts_with("rx bitrate:") {
                        rx_rate = trimmed
                            .trim_start_matches("rx bitrate:")
                            .split_whitespace()
                            .take(2)
                            .collect::<Vec<&str>>()
                            .join(" ");
                    } else if trimmed.starts_with("tx bitrate:") {
                        tx_rate = trimmed
                            .trim_start_matches("tx bitrate:")
                            .split_whitespace()
                            .take(2)
                            .collect::<Vec<&str>>()
                            .join(" ");
                    }
                }
                if !tx_rate.is_empty() {
                    let tx_clean = tx_rate.replace("MBit/s", "Mbps");
                    let rx_clean = rx_rate.replace("MBit/s", "Mbps");
                    active_speed = format!("↓{} ↑{}", rx_clean, tx_clean);
                } else {
                    active_speed = rate.clone();
                }
            }

            if let Some(existing) = networks.iter_mut().find(|n| n.ssid == ssid) {
                if active {
                    existing.active = true;
                    existing.bssid = bssid;
                    existing.rate = rate;
                    existing.signal = signal;
                } else if !existing.active && signal > existing.signal {
                    existing.signal = signal;
                }
            } else {
                seen_ssids.insert(ssid.clone());
                let (saved, autoconnect) = match saved_connections.get(&ssid) {
                    Some(&auto) => (true, auto),
                    None => (false, false),
                };
                networks.push(WifiNetwork {
                    ssid,
                    bssid,
                    signal,
                    rate,
                    security,
                    active,
                    saved,
                    autoconnect,
                });
            }
        }

        if !connected {
            let dev_out = run_out(&nmcli(), &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev"]);
            for line in dev_out.lines() {
                let parts = split_nmcli_t_line(line);
                if parts.len() >= 4
                    && parts[1] == "wifi"
                    && parts[2].starts_with("connected")
                    && !parts[3].is_empty()
                {
                    connected = true;
                    if active_ssid.is_empty() {
                        active_ssid = parts[3].clone();
                    }
                }
            }
        }

        if connected && !active_ssid.is_empty() {
            let mut found = false;
            for net in networks.iter_mut() {
                if net.ssid == active_ssid {
                    net.active = true;
                    found = true;
                }
            }
            if !found {
                let (saved, autoconnect) = match saved_connections.get(&active_ssid) {
                    Some(&auto) => (true, auto),
                    None => (true, true),
                };
                networks.push(WifiNetwork {
                    ssid: active_ssid.clone(),
                    bssid: details.bssid.clone(),
                    signal: active_signal,
                    rate: active_speed.clone(),
                    security: details.security.clone(),
                    active: true,
                    saved,
                    autoconnect,
                });
            }
        }
    }

    networks.sort_by(|a, b| {
        if a.active != b.active {
            b.active.cmp(&a.active)
        } else {
            b.signal.cmp(&a.signal)
        }
    });

    if connected && let Some(dev) = wifi_device.as_deref() {
        let dev_info = run_out(&nmcli(), &["dev", "show", dev]);
        for line in dev_info.lines() {
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim();
                let val = parts[1].trim().to_string();
                match key {
                    "IP4.ADDRESS[1]" => {
                        let ip_parts: Vec<&str> = val.split('/').collect();
                        if !ip_parts.is_empty() {
                            details.ip_address = ip_parts[0].to_string();
                            if ip_parts.len() > 1
                                && let Ok(cidr) = ip_parts[1].parse::<u32>()
                                && let Some(mask) = cidr_to_netmask(cidr)
                            {
                                details.subnet = mask;
                            }
                        }
                    }
                    "IP4.GATEWAY" => details.gateway = val,
                    "IP4.DNS[1]" => {
                        if details.dns.is_empty() {
                            details.dns = val;
                        } else {
                            details.dns = format!("{}, {}", details.dns, val);
                        }
                    }
                    "IP4.DNS[2]" if !details.dns.is_empty() => {
                        details.dns = format!("{}, {}", details.dns, val);
                    }
                    _ => {}
                }
            }
        }
    }

    let conns_out = run_out(&nmcli(), &["-t", "-f", "NAME,TYPE,DEVICE,STATE", "connection", "show"]);
    for line in conns_out.lines() {
        let parts = split_nmcli_t_line(line);
        if parts.len() >= 4 {
            let name = parts[0].clone();
            let conn_type = parts[1].to_lowercase();
            let device = parts[2].clone();
            let state = parts[3].to_lowercase();
            let active = state.contains("activated");
            let is_vpn = conn_type.contains("vpn")
                || conn_type.contains("wireguard")
                || conn_type.contains("tun");
            if is_vpn && !device.is_empty() && device != "lo" && !device.starts_with("virbr") {
                vpns.push(VpnConnection {
                    name,
                    vpn_type: conn_type,
                    device,
                    active,
                });
            }
        }
    }

    let warp_out = run_out(&warp_cli(), &["status"]);
    let warp_connected = warp_out
        .lines()
        .any(|l| l.to_lowercase().contains("status update: connected"));

    let ts_out = run_out(&tailscale(), &["status", "--json"]);
    let mut tailscale_connected = false;
    let mut tailscale_ip = String::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&ts_out) {
        if v.get("BackendState").and_then(|s| s.as_str()) == Some("Running") {
            tailscale_connected = true;
        }
        if let Some(ip) = v
            .get("Self")
            .and_then(|s| s.get("TailscaleIPs"))
            .and_then(|i| i.as_array())
            .and_then(|ips| ips.first())
            .and_then(|i| i.as_str())
        {
            tailscale_ip = ip.to_string();
        }
    }

    let status = NetworkStatus {
        wifi_enabled,
        airplane_mode,
        connected,
        active_ssid,
        active_signal,
        active_speed,
        wifi_device: wifi_device.unwrap_or_default(),
        warp_connected,
        tailscale_connected,
        tailscale_ip,
        details,
        networks,
        vpns,
    };

    // Cache.
    let cache_path = cache_dir().join("network_status.json");
    let _ = super::common::atomic_write_json(&cache_path, &status);

    status
}

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
