//! Collect wifi networks, VPNs, details, warp/tailscale.

use super::super::common::{
    active_wifi_device, cache_dir, cidr_to_netmask, iw, nmcli, run_out, split_nmcli_t_line,
    tailscale, warp_cli,
};
use super::types::{ConnectionDetails, NetworkStatus, VpnConnection, WifiNetwork};
use super::wifi::{is_airplane_mode, is_wifi_enabled, parse_wifi_line, wifi_saved_connections};

/// Join every `IP4.DNS[*]` value from a `dev show` listing into the
/// single display string.
fn join_dns(dns: &[String]) -> String {
    dns.join(", ")
}

/// Merge every scan row for the same SSID into one entry, preferring
/// the active BSSID and otherwise keeping the strongest signal, so
/// duplicate SSIDs (2.4/5/6 GHz) stay a single row.
fn index_of_ssid(rows: &[WifiNetwork], ssid: &str) -> Option<usize> {
    rows.iter().position(|n| n.ssid == ssid)
}

/// Pure: format an `iw link` bitrate dump as `↓rx ↑tx`, or fall back
/// to the scan-reported rate.
pub(crate) fn link_speed(out: &str, fallback_rate: &str) -> String {
    let (mut tx, mut rx) = (String::new(), String::new());
    for l in out.lines() {
        let t = l.trim();
        if let Some(v) = t.strip_prefix("rx bitrate:") {
            rx = v.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        } else if let Some(v) = t.strip_prefix("tx bitrate:") {
            tx = v.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        }
    }
    if tx.is_empty() {
        return fallback_rate.to_string();
    }
    format!(
        "↓{} ↑{}",
        rx.replace("MBit/s", "Mbps"),
        tx.replace("MBit/s", "Mbps")
    )
}

/// Pure: first connected ethernet device from a
/// `device status` listing, if any.
pub(crate) fn parse_wired_device(out: &str) -> String {
    for line in out.lines() {
        let parts = split_nmcli_t_line(line);
        if parts.len() >= 3 && parts[1] == "ethernet" && parts[2].starts_with("connected") {
            return parts[0].clone();
        }
    }
    String::new()
}

/// Pure: accumulate every `IP4.DNS[*]` value from a `dev show` dump.
pub(crate) fn parse_dev_dns(out: &str) -> Vec<String> {
    let mut v = Vec::new();
    for line in out.lines() {
        let Some((k, val)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim();
        let val = val.trim().to_string();
        if k.starts_with("IP4.DNS") && !val.is_empty() {
            v.push(val);
        }
    }
    v
}

pub(crate) fn collect(force_rescan: bool) -> NetworkStatus {
    let wifi_enabled = is_wifi_enabled();
    let airplane_mode = is_airplane_mode();
    let wifi_device = active_wifi_device();

    // One `device status` read feeds both the wired link and the
    // connected fallback — no separate query per fact.
    let dev_status_out = run_out(
        &nmcli(),
        &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev"],
    );
    let wired = parse_wired_device(&dev_status_out);

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

        // One `iw link` read for the active link, outside the scan
        // loop: the old code ran it every time an "active" line
        // appeared and fell back to a hardcoded "wlo1" when the
        // device was unknown.
        let active_link_speed = wifi_device
            .as_deref()
            .map(|dev| run_out(&iw(), &["dev", dev, "link"]));

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
                active_speed = active_link_speed
                    .as_deref()
                    .map(|link| link_speed(link, &rate))
                    .unwrap_or_else(|| rate.clone());
            }

            if let Some(i) = index_of_ssid(&networks, &ssid) {
                let existing = &mut networks[i];
                if active {
                    existing.active = true;
                    existing.bssid = bssid;
                    existing.rate = rate;
                    existing.signal = signal;
                } else if !existing.active && signal > existing.signal {
                    existing.signal = signal;
                }
            } else {
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
            let dev_out = dev_status_out.clone();
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
            let Some((key, val)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim().to_string();
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
                _ => {}
            }
        }
        // Every DNS entry, not just the first two.
        details.dns = join_dns(&parse_dev_dns(&dev_info));
    }

    let conns_out = run_out(
        &nmcli(),
        &["-t", "-f", "NAME,TYPE,DEVICE,STATE", "connection", "show"],
    );
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
        wired: wired.clone(),
        warp_connected,
        tailscale_connected,
        tailscale_ip,
        details,
        networks,
        vpns,
    };

    // Cache.
    let cache_path = cache_dir().join("network_status.json");
    let _ = crate::status::common::atomic_write_json(&cache_path, &status);

    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wired_device_picks_first_connected_ethernet() {
        let out = "wlo1:wifi:connected:0 Connected to MyNet\neth0:ethernet:connected:0 Connected to Wired connection 1\nlo:loopback:unconnected:\n";
        assert_eq!(parse_wired_device(out), "eth0");
    }

    #[test]
    fn wired_device_empty_when_disconnected() {
        assert_eq!(parse_wired_device("eth0:ethernet:disconnected:\n"), "");
        assert_eq!(parse_wired_device(""), "");
        assert_eq!(parse_wired_device("garbage\n"), "");
    }

    #[test]
    fn dns_accumulates_every_entry() {
        let out = "IP4.ADDRESS[1]:  192.168.1.2/24\nIP4.DNS[1]:  1.1.1.1\nIP4.DNS[2]:  1.0.0.1\nIP4.DNS[3]:  8.8.8.8\nIP4.DNS[4]:  \n";
        assert_eq!(parse_dev_dns(out), vec!["1.1.1.1", "1.0.0.1", "8.8.8.8"]);
    }

    #[test]
    fn link_speed_formats_both_directions() {
        let out = "Connected to abcdef\n\tConnected time: 100\nrx bitrate: 540.0 MBit/s\ntx bitrate: 120.0 MBit/s\n";
        assert_eq!(link_speed(out, "540 Mbit/s"), "↓540.0 Mbps ↑120.0 Mbps");
    }

    #[test]
    fn link_speed_falls_back_to_scan_rate() {
        assert_eq!(link_speed("some other output", "480 Mbit/s"), "480 Mbit/s");
    }
}
