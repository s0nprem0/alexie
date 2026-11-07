//! Collect wifi networks, VPNs, details, warp/tailscale.

use super::super::common::{
    active_wifi_device, cache_dir, cidr_to_netmask, iw, nmcli, run_out, split_nmcli_t_line,
    tailscale, warp_cli,
};
use super::types::{ConnectionDetails, NetworkStatus, VpnConnection, WifiNetwork};
use super::wifi::{is_airplane_mode, is_wifi_enabled, parse_wifi_line, wifi_saved_connections};
use std::collections::HashSet;
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
            let dev_out = run_out(
                &nmcli(),
                &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev"],
            );
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
