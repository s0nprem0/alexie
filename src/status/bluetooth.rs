//! Bluetooth status (wabi `get_bluetooth_status` JSON).

use super::common::{print_json, run_out, bluetoothctl, upower};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::time::Duration;

#[derive(Serialize, Clone, Debug)]
pub struct BluetoothDevice {
    pub name: String,
    pub address: String,
    pub connected: bool,
    pub paired: bool,
    pub trusted: bool,
    pub battery: Option<f64>,
    pub device_type: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct BluetoothStatus {
    pub enabled: bool,
    pub devices: Vec<BluetoothDevice>,
}

fn is_bluetooth_enabled() -> bool {
    let out = run_out(&bluetoothctl(), &["show"]);
    for line in out.lines() {
        if line.contains("Powered:") {
            return line.contains("yes");
        }
    }
    false
}

pub(crate) fn parse_upower_batteries_out(out: &str) -> HashMap<String, f64> {
    let mut batteries = HashMap::new();
    let mut current_address = String::new();
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Device:") || trimmed.starts_with("native-path:") {
            // New device block starts; keep the just-seen native-path
            // handling below, but clear serial from previous block.
            if trimmed.starts_with("Device:") {
                current_address = String::new();
            }
        }
        if let Some(serial) = trimmed.strip_prefix("serial:") {
            current_address = serial.trim().to_uppercase();
        } else if trimmed.starts_with("native-path:") {
            current_address = trimmed
                .trim_start_matches("native-path:")
                .trim()
                .replace('_', ":")
                .to_uppercase();
        }
        if trimmed.starts_with("percentage:") && !current_address.is_empty() {
            let pct_str = trimmed
                .trim_start_matches("percentage:")
                .trim()
                .trim_end_matches('%');
            if let Ok(pct) = pct_str.parse::<f64>() {
                batteries.insert(current_address.clone(), pct);
            }
        }
    }
    batteries
}

fn parse_upower_batteries() -> HashMap<String, f64> {
    let out = run_out(&upower(), &["-d"]);
    parse_upower_batteries_out(&out)
}

fn get_battery_for_device(address: &str, upower_batteries: &HashMap<String, f64>) -> Option<f64> {
    if let Some(pct) = upower_batteries.get(&address.to_uppercase()) {
        return Some(*pct);
    }
    for (device_key, pct) in upower_batteries {
        if device_key.contains(&address.to_uppercase()) {
            return Some(*pct);
        }
    }

    let bat_out = run_out(&bluetoothctl(), &["battery-info", address]);
    for line in bat_out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Battery Percentage:")
            && let Some(start) = trimmed.rfind('(')
            && let Some(end) = trimmed.rfind(')')
        {
            let pct_str = &trimmed[start + 1..end].trim_end_matches('%');
            if let Ok(pct) = pct_str.parse::<f64>() {
                return Some(pct);
            }
        }
    }

    None
}

pub(crate) fn classify_device_type(name: &str, info: &str) -> String {
    let info_lower = info.to_lowercase();
    if info_lower.contains("icon: audio")
        || info_lower.contains("audio sink")
        || info_lower.contains("audio source")
        || info_lower.contains("headset")
        || info_lower.contains("handsfree")
    {
        return "audio".to_string();
    }
    if info_lower.contains("icon: input-keyboard") || info_lower.contains("keyboard") {
        return "keyboard".to_string();
    }
    if info_lower.contains("icon: input-mouse") || info_lower.contains("mouse") {
        return "mouse".to_string();
    }
    if info_lower.contains("icon: input-gaming") || info_lower.contains("gamepad") {
        return "controller".to_string();
    }

    let lower = name.to_lowercase();
    if lower.contains("headphone")
        || lower.contains("headset")
        || lower.contains("earbuds")
        || lower.contains("earphone")
        || lower.contains("airpods")
        || lower.contains("buds")
        || lower.contains("qc")
        || lower.contains("wh-")
        || lower.contains("wf-")
        || lower.contains("beats")
        || lower.contains("jabra")
        || lower.contains("sony")
    {
        "audio".to_string()
    } else if lower.contains("keyboard") || lower.contains("keychron") {
        "keyboard".to_string()
    } else if lower.contains("mouse")
        || lower.contains("logitech")
        || lower.contains("g502")
        || lower.contains("g pro")
        || lower.contains("deathadder")
    {
        "mouse".to_string()
    } else if lower.contains("controller")
        || lower.contains("xbox")
        || lower.contains("ps5")
        || lower.contains("dualshock")
        || lower.contains("dualsense")
    {
        "controller".to_string()
    } else {
        "device".to_string()
    }
}

fn do_scan() {
    let _ = Command::new(crate::proc::resolve_tool(&bluetoothctl()))
        .args(["scan", "on"])
        .output();
    std::thread::sleep(Duration::from_millis(1500));
    let _ = Command::new(crate::proc::resolve_tool(&bluetoothctl()))
        .args(["scan", "off"])
        .output();
}

pub(crate) fn parse_devices_out(out: &str) -> Vec<(String, String)> {
    let mut addresses = Vec::new();
    for line in out.lines() {
        if line.starts_with("Device ") {
            let rest = line.trim_start_matches("Device ");
            if let Some(space_idx) = rest.find(' ') {
                let addr = rest[..space_idx].to_string();
                let name = rest[space_idx + 1..].to_string();
                addresses.push((addr, name));
            }
        }
    }
    addresses
}

fn get_devices(should_scan: bool) -> Vec<BluetoothDevice> {
    let mut devices = Vec::new();
    let mut known_addresses = HashSet::new();
    let upower_batteries = parse_upower_batteries();

    let devices_out = run_out(&bluetoothctl(), &["devices"]);
    let mut addresses: Vec<(String, String)> = Vec::new();

    for (addr, name) in parse_devices_out(&devices_out) {
        known_addresses.insert(addr.clone());
        addresses.push((addr, name));
    }

    if should_scan {
        do_scan();
        let devices_out_after = run_out(&bluetoothctl(), &["devices"]);
        for (addr, name) in parse_devices_out(&devices_out_after) {
            if !known_addresses.contains(&addr) {
                known_addresses.insert(addr.clone());
                addresses.push((addr, name));
            }
        }
    }

    for (addr, name) in &addresses {
        let info_out = run_out(&bluetoothctl(), &["info", addr]);
        let mut connected = false;
        let mut paired = false;
        let mut trusted = false;

        for line in info_out.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Connected:") {
                connected = trimmed.contains("yes");
            } else if trimmed.starts_with("Paired:") {
                paired = trimmed.contains("yes");
            } else if trimmed.starts_with("Trusted:") {
                trusted = trimmed.contains("yes");
            }
        }

        let battery = get_battery_for_device(addr, &upower_batteries);
        let device_type = classify_device_type(name, &info_out);

        devices.push(BluetoothDevice {
            name: name.clone(),
            address: addr.clone(),
            connected,
            paired,
            trusted,
            battery,
            device_type,
        });
    }

    devices.sort_by(|a, b| {
        if a.connected != b.connected {
            b.connected.cmp(&a.connected)
        } else if a.battery.is_some() != b.battery.is_some() {
            b.battery.is_some().cmp(&a.battery.is_some())
        } else {
            a.name.cmp(&b.name)
        }
    });

    devices
}

pub(crate) fn collect(should_scan: bool) -> BluetoothStatus {
    let enabled = is_bluetooth_enabled();
    let devices = if enabled {
        get_devices(should_scan)
    } else {
        Vec::new()
    };
    BluetoothStatus { enabled, devices }
}

pub(crate) fn render_text(s: &BluetoothStatus) -> String {
    if !s.enabled {
        return "bluetooth off".to_string();
    }
    if s.devices.is_empty() {
        return "bluetooth on, no devices".to_string();
    }
    let rows: Vec<Vec<String>> = s
        .devices
        .iter()
        .map(|d| {
            vec![
                d.name.clone(),
                d.address.clone(),
                d.device_type.clone(),
                if d.connected { "yes".into() } else { "no".into() },
                if d.paired { "yes".into() } else { "no".into() },
                d.battery.map(|b| format!("{}%", b.round() as i64)).unwrap_or_else(|| "-".into()),
            ]
        })
        .collect();
    crate::table::render(&["Name", "Address", "Type", "Connected", "Paired", "Battery"], &rows)
}

pub(crate) fn handle(args: &[String]) -> String {
    let want_json = args.iter().any(|a| a == "--json");
    let do_scan = args.iter().any(|a| a == "--scan");
    for a in args {
        if a.starts_with("--") && a != "--json" && a != "--scan" {
            return format!("ERR unknown flag: {}", a);
        }
    }
    let status = collect(do_scan);
    if want_json {
        print_json(&status)
    } else {
        render_text(&status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_info_first() {
        assert_eq!(classify_device_type("Unknown", "Icon: audio-headset"), "audio");
        assert_eq!(classify_device_type("Foo", "some keyboard info"), "keyboard");
    }

    #[test]
    fn classify_by_name_fallback() {
        assert_eq!(classify_device_type("Sony WH-1000XM5", ""), "audio");
        assert_eq!(classify_device_type("Keychron K8", ""), "keyboard");
        assert_eq!(classify_device_type("Logitech G502", ""), "mouse");
        assert_eq!(classify_device_type("Xbox Controller", ""), "controller");
        assert_eq!(classify_device_type("Some Gadget", ""), "device");
    }

    #[test]
    fn parse_devices_line() {
        let out = "Device AA:BB:CC:DD:EE:FF My Device\nDevice 11:22:33:44:55:66 Other\nNot a device";
        let devs = parse_devices_out(out);
        assert_eq!(devs.len(), 2);
        assert_eq!(devs[0].1, "My Device");
    }

    #[test]
    fn parse_upower_native_path() {
        // Native-path keys are full paths; match by contains().
        let out = "native-path: /sys/devices/pci0000:00/0000:00:14.0/usb1/1-2/AA_BB_CC_DD_EE_FF\npercentage: 75%\n";
        let map = parse_upower_batteries_out(out);
        assert!(map.values().any(|v| *v == 75.0));
        assert!(map.keys().any(|k| k.contains("AA:BB:CC:DD:EE:FF")));
    }

    #[test]
    fn parse_upower_serial() {
        let out = "serial: AA:BB:CC:DD:EE:FF\npercentage: 75%\n";
        let map = parse_upower_batteries_out(out);
        assert_eq!(map.get("AA:BB:CC:DD:EE:FF"), Some(&75.0));
    }

    #[test]
    fn handle_json_is_parseable() {
        let out = handle(&["--json".to_string()]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("bluetooth --json must be JSON");
        assert!(v.get("enabled").is_some());
        assert!(v.get("devices").is_some());
    }

    #[test]
    fn handle_unknown_flag_fails_fast() {
        let out = handle(&["--bogus".to_string()]);
        assert!(out.starts_with("ERR unknown flag:"));
    }
}
