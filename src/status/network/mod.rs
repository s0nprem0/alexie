//! Network status (wabi `get_network_status` JSON).
//!
//! Split from a single 503-line `network.rs`:
//! - `types` — WifiNetwork/VpnConnection/ConnectionDetails/NetworkStatus
//! - `wifi` — radio state, saved connections, line parsing
//! - `collect` — wifi networks, VPNs, details, warp/tailscale
//!
//! `to_json()`/`render_text()`/`handle()` below just orchestrate.

pub(crate) mod collect;
pub(crate) mod types;
pub(crate) mod wifi;

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
        if s.active_ssid.is_empty() {
            "-"
        } else {
            &s.active_ssid
        },
        s.active_signal,
        if s.active_speed.is_empty() {
            "-"
        } else {
            &s.active_speed
        },
    ));
    if !s.details.ip_address.is_empty() {
        out.push_str(&format!(
            "ip {}  gw {}  dns {}  subnet {}\n",
            s.details.ip_address,
            if s.details.gateway.is_empty() {
                "-"
            } else {
                &s.details.gateway
            },
            if s.details.dns.is_empty() {
                "-"
            } else {
                &s.details.dns
            },
            if s.details.subnet.is_empty() {
                "-"
            } else {
                &s.details.subnet
            },
        ));
    }
    if !s.vpns.is_empty() {
        let rows: Vec<Vec<String>> = s
            .vpns
            .iter()
            .map(|v| {
                vec![
                    v.name.clone(),
                    v.vpn_type.clone(),
                    v.device.clone(),
                    if v.active { "yes".into() } else { "no".into() },
                ]
            })
            .collect();
        out.push_str(&crate::table::render(
            &["VPN", "Type", "Device", "Active"],
            &rows,
        ));
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
        out.push_str(&crate::table::render(
            &["SSID", "Signal", "Rate", "Security", "Active", "Saved"],
            &rows,
        ));
    }
    if out.trim().is_empty() {
        out.push_str("network: no data (nmcli unavailable or wifi off)");
    }
    out.trim_end().to_string()
}

pub(crate) fn handle(args: &[String]) -> String {
    // Subcommands that write (band/dns) must be checked first; the bare
    // `network [--json] [--rescan]` path is read-only status.
    if !args.is_empty() {
        match args[0].as_str() {
            "band" => return handle_band(&args[1..]),
            "dns" => return handle_dns(&args[1..]),
            _ => {}
        }
    }

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

// ---- band ----

fn band_from_nm(v: &str) -> &'static str {
    match v {
        "bg" => "2.4",
        "a" => "5",
        "6GHz" => "6",
        _ => "auto",
    }
}

fn band_to_nm(target: &str) -> Option<&'static str> {
    match target {
        "2.4" => Some("bg"),
        "5" => Some("a"),
        "6" => Some("6GHz"),
        _ => None,
    }
}

fn band_for_freq(mhz: u32) -> Option<&'static str> {
    if (2400..2500).contains(&mhz) {
        Some("2.4")
    } else if (4900..5925).contains(&mhz) {
        Some("5")
    } else if (5925..7125).contains(&mhz) {
        Some("6")
    } else {
        None
    }
}

/// Read the active link's freq + SSID from `dev wifi list`.
fn read_active(device: &str) -> Option<(u32, String)> {
    use crate::status::common::{nmcli, split_nmcli_t_line};
    let (_, out, _) = crate::proc::run(
        &nmcli(),
        &["-t", "-e", "no", "-f", "IN-USE,FREQ,SSID", "dev", "wifi", "list", "ifname", device, "--rescan", "no"],
    );
    for line in out.lines() {
        let parts = split_nmcli_t_line(line);
        if parts.len() >= 3 && parts[0] == "*" {
            let mhz: u32 = parts[1].trim().parse().ok()?;
            // SSID may contain colons; rejoin the rest.
            return Some((mhz, parts[2..].join(":")));
        }
    }
    None
}

/// Bands visible for the SSID, plus the current one.
fn available_bands(device: &str, ssid: &str) -> Vec<&'static str> {
    use crate::status::common::{nmcli, split_nmcli_t_line};
    let (_, out, _) = crate::proc::run(
        &nmcli(),
        &["-e", "no", "-g", "FREQ,SSID", "dev", "wifi", "list", "ifname", device, "--rescan", "no"],
    );
    let mut bands = Vec::new();
    for line in out.lines() {
        let parts = split_nmcli_t_line(line);
        if parts.len() < 2 {
            continue;
        }
        if parts[1..].join(":") != ssid {
            continue;
        }
        if let Ok(mhz) = parts[0].trim().parse::<u32>() {
            if let Some(b) = band_for_freq(mhz) {
                if !bands.contains(&b) {
                    bands.push(b);
                }
            }
        }
    }
    bands
}

fn handle_band(args: &[String]) -> String {
    use crate::status::common::{nm_get, nmcli, wifi_device, wifi_profile};

    match args.len() {
        0 => {
            // print_status
            let Some(device) = wifi_device() else { return String::new() };
            let Some((mhz, ssid)) = read_active(&device) else { return String::new() };
            if ssid.is_empty() {
                return String::new();
            }
            let band = band_for_freq(mhz).unwrap_or("auto");
            let mut available = available_bands(&device, &ssid);
            if !available.contains(&band) {
                available.push(band);
            }
            available.sort_by(|a, b| {
                let ka = a.parse::<f64>().unwrap_or(0.0);
                let kb = b.parse::<f64>().unwrap_or(0.0);
                ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut out = format!("band\t{}\n", band);
            out.push_str(&format!("available\t{}\n", available.join(" ")));
            if let Some(profile) = wifi_profile(&device) {
                let selected = band_from_nm(&nm_get(&["802-11-wireless.band", "connection", "show", &profile]));
                out.push_str(&format!("selected\t{}\n", selected));
            }
            out
        }
        1 => {
            let target = args[0].as_str();
            if target != "auto" && band_to_nm(target).is_none() {
                return format!("ERR invalid band {}", target);
            }
            let Some(device) = wifi_device() else {
                return "ERR no connected Wi-Fi device".to_string();
            };
            let Some(profile) = wifi_profile(&device) else {
                return "ERR no active Wi-Fi profile".to_string();
            };

            let desired = if target == "auto" {
                String::new()
            } else {
                let Some((mhz, ssid)) = read_active(&device) else {
                    return "ERR could not read the active link".to_string();
                };
                let cur_band = band_for_freq(mhz).unwrap_or("auto");
                let mut avail = available_bands(&device, &ssid);
                if !avail.contains(&cur_band) {
                    avail.push(cur_band);
                }
                if !avail.contains(&target) {
                    return format!("ERR {}GHz is not available on this network", target);
                }
                band_to_nm(target).unwrap_or("").to_string()
            };

            let previous = nm_get(&["802-11-wireless.band", "connection", "show", &profile]);
            if previous == desired {
                return "OK".to_string();
            }

            let (code, _, err) = crate::proc::run(
                &nmcli(),
                &["connection", "modify", &profile, "802-11-wireless.band", &desired],
            );
            if code != 0 {
                return format!("ERR nmcli connection modify: {}", err.trim());
            }
            let (code, _, _) = crate::proc::run(&nmcli(), &["connection", "up", &profile]);
            if code != 0 {
                let _ = crate::proc::run(
                    &nmcli(),
                    &["connection", "modify", &profile, "802-11-wireless.band", &previous],
                );
                let _ = crate::proc::run(&nmcli(), &["connection", "up", &profile]);
                return format!("ERR could not connect on {}; reverted", target);
            }
            "OK".to_string()
        }
        _ => "ERR usage: network band [auto|2.4|5|6]".to_string(),
    }
}

// ---- dns ----

fn handle_dns(args: &[String]) -> String {
    use crate::status::common::{nm_get, wifi_device, wifi_profile};

    if args.is_empty() {
        let Some(device) = wifi_device() else { return String::new() };
        let Some(profile) = wifi_profile(&device) else { return String::new() };
        let v4 = nm_get(&["ipv4.dns", "connection", "show", &profile]);
        let v6 = nm_get(&["ipv6.dns", "connection", "show", &profile]);
        return format!("ipv4\t{}\nipv6\t{}\n", v4, v6);
    }
    if args[0] == "set" && args.len() == 3 {
        return set_dns(&args[1], &args[2]);
    }
    "ERR usage: network dns [set <ipv4> <ipv6>]".to_string()
}

fn set_dns(v4: &str, v6: &str) -> String {
    use crate::status::common::{nm_get, nmcli, wifi_device, wifi_profile};

    let nmcli = nmcli();

    let Some(device) = wifi_device() else {
        return "ERR no connected Wi-Fi device".to_string();
    };
    let Some(profile) = wifi_profile(&device) else {
        return "ERR no active Wi-Fi profile".to_string();
    };

    let prev4 = nm_get(&["ipv4.dns", "connection", "show", &profile]);
    let prev6 = nm_get(&["ipv6.dns", "connection", "show", &profile]);
    let prev_ign4 = {
        let v = nm_get(&["ipv4.ignore-auto-dns", "connection", "show", &profile]);
        if v.is_empty() { "no".to_string() } else { v }
    };
    let prev_ign6 = {
        let v = nm_get(&["ipv6.ignore-auto-dns", "connection", "show", &profile]);
        if v.is_empty() { "no".to_string() } else { v }
    };

    let ign4 = if v4.is_empty() { "no" } else { "yes" };
    let ign6 = if v6.is_empty() { "no" } else { "yes" };

    if prev4 == v4 && prev6 == v6 && prev_ign4 == ign4 && prev_ign6 == ign6 {
        return "OK".to_string();
    }

    let restore = |profile: &str, p4: &str, pi4: &str, p6: &str, pi6: &str| {
        let _ = crate::proc::run(
            &nmcli,
            &["connection", "modify", profile,
              "ipv4.dns", p4, "ipv4.ignore-auto-dns", pi4,
              "ipv6.dns", p6, "ipv6.ignore-auto-dns", pi6],
        );
        let _ = crate::proc::run(&nmcli, &["connection", "up", profile]);
    };

    let (code, _, err) = crate::proc::run(
        &nmcli,
        &["connection", "modify", &profile,
          "ipv4.dns", v4, "ipv4.ignore-auto-dns", ign4,
          "ipv6.dns", v6, "ipv6.ignore-auto-dns", ign6],
    );
    if code != 0 {
        restore(&profile, &prev4, &prev_ign4, &prev6, &prev_ign6);
        return format!("ERR could not change DNS; reverted: {}", err.trim());
    }
    let (code, _, _) = crate::proc::run(&nmcli, &["connection", "up", &profile]);
    if code != 0 {
        restore(&profile, &prev4, &prev_ign4, &prev6, &prev_ign6);
        return "ERR could not reconnect with the new DNS; reverted".to_string();
    }
    "OK".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wifi::parse_saved_connections;

    #[test]
    fn saved_map_matches_ssid_not_profile_name() {
        // Profile NAME differs from its SSID; the map must be keyed by
        // SSID or the UI re-asks for the password on every reconnect.
        let out = "Home:MyNetwork:yes\nWork:Office AP:no\nWired:\n".to_string();
        let map = parse_saved_connections(&out);
        assert_eq!(map.get("MyNetwork"), Some(&true));
        assert_eq!(map.get("Office AP"), Some(&false));
        assert!(map.get("Home").is_none(), "must not key by profile NAME");
        assert!(map.get("Work").is_none(), "must not key by profile NAME");
    }

    #[test]
    fn saved_map_handles_escaped_colons_and_trims() {
        let out = "My\\:Profile:My\\:Wifi:yes\nPadded:  Spaced  :yes\n: :no\nshort\n".to_string();
        let map = parse_saved_connections(&out);
        assert_eq!(map.get("My:Wifi"), Some(&true));
        assert_eq!(map.get("Spaced"), Some(&true));
        // Empty SSID (non-wireless row) and short rows are skipped.
        assert!(!map.contains_key(""));
        assert_eq!(map.len(), 2);
    }

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
