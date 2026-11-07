//! Shared helpers for status.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) use crate::proc::tool;

pub(crate) fn nmcli() -> String {
    tool("NMCLI", "nmcli")
}
pub(crate) fn rfkill() -> String {
    tool("RFKILL", "rfkill")
}
pub(crate) fn iw() -> String {
    tool("IW", "iw")
}
pub(crate) fn warp_cli() -> String {
    tool("WARP_CLI", "warp-cli")
}
pub(crate) fn tailscale() -> String {
    tool("TAILSCALE", "tailscale")
}
pub(crate) fn pactl() -> String {
    tool("PACTL", "pactl")
}

pub(crate) fn wpctl() -> String {
    tool("WPCTL", "wpctl")
}
pub(crate) fn playerctl() -> String {
    tool("PLAYERCTL", "playerctl")
}
pub(crate) fn bluetoothctl() -> String {
    tool("BLUETOOTHCTL", "bluetoothctl")
}
pub(crate) fn upower() -> String {
    tool("UPOWER", "upower")
}
pub(crate) fn nvidia_smi() -> String {
    tool("NVIDIA_SMI", "nvidia-smi")
}
pub(crate) fn inxi() -> String {
    tool("INXI", "inxi")
}
pub(crate) fn lsblk() -> String {
    tool("LSBLK", "lsblk")
}

/// Run a command, return trimmed stdout on success else "".
/// Run, return trimmed stdout or empty.
pub(crate) fn run_out(cmd: &str, args: &[&str]) -> String {
    let (code, out, _) = crate::proc::run(cmd, args);
    if code == 0 {
        out.trim().to_string()
    } else {
        String::new()
    }
}

pub(crate) fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub(crate) fn atomic_write_json<T: serde::Serialize>(
    path: &Path,
    value: &T,
) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, path)
}

pub(crate) fn print_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn cache_dir() -> PathBuf {
    let base = env::var("XDG_CACHE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.cache", home)
    });
    PathBuf::from(base).join("anixie")
}

// Pure helpers.

pub(crate) fn parse_percent(value: &str) -> Option<i64> {
    let pct = value.split('%').next()?.trim();
    pct.parse::<f64>().ok().map(|v| v.round() as i64)
}

pub(crate) fn split_nmcli_t_line(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                current.push(next);
            }
        } else if c == ':' {
            parts.push(current);
            current = String::new();
        } else {
            current.push(c);
        }
    }
    parts.push(current);
    parts
}

pub(crate) fn cidr_to_netmask(prefix: u32) -> Option<String> {
    if prefix > 32 {
        return None;
    }
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Some(format!(
        "{}.{}.{}.{}",
        (mask >> 24) & 0xFF,
        (mask >> 16) & 0xFF,
        (mask >> 8) & 0xFF,
        mask & 0xFF
    ))
}

pub(crate) fn active_wifi_device() -> Option<String> {
    let out = {
        let (code, o, _) = crate::proc::run(&nmcli(), &["-t", "-f", "DEVICE,TYPE,STATE", "dev"]);
        if code == 0 {
            o
        } else {
            return None;
        }
    };
    for line in out.lines() {
        let parts = split_nmcli_t_line(line);
        if parts.len() >= 3 && parts[1] == "wifi" && parts[2].contains("connected") {
            return Some(parts[0].clone());
        }
    }
    for line in out.lines() {
        let parts = split_nmcli_t_line(line);
        if parts.len() >= 2 && parts[1] == "wifi" {
            return Some(parts[0].clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_escaped_nmcli_fields() {
        let fields = split_nmcli_t_line("yes:My\\:Wifi:AA\\:BB\\:CC:80:540 Mbit/s:WPA2");
        assert_eq!(fields[1], "My:Wifi");
        assert_eq!(fields[2], "AA:BB:CC");
    }

    #[test]
    fn converts_cidr_to_netmask() {
        assert_eq!(cidr_to_netmask(24).as_deref(), Some("255.255.255.0"));
        assert_eq!(cidr_to_netmask(0).as_deref(), Some("0.0.0.0"));
        assert!(cidr_to_netmask(33).is_none());
    }

    #[test]
    fn parses_decimal_percent() {
        assert_eq!(parse_percent("12.5%"), Some(13));
        assert_eq!(parse_percent("100%"), Some(100));
        assert_eq!(parse_percent("nope"), None);
    }

    #[test]
    fn nmcli_split_empty_and_colons() {
        assert_eq!(split_nmcli_t_line(""), vec!["".to_string()]);
        assert_eq!(
            split_nmcli_t_line("a:b:c"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn run_out_missing_binary_is_empty() {
        // Must not panic or block; returns "".
        assert_eq!(run_out("alexie-no-such-binary-xyz", &[]), "");
    }
}
