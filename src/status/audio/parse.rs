//! pactl parsing: volume, sample rate, icons, info.

use super::super::common::{pactl, parse_percent};
pub(crate) fn get_pactl_json(category: &str) -> serde_json::Value {
    let (code, out, _) = crate::proc::run(&pactl(), &["-f", "json", "list", category]);
    if code != 0 {
        return serde_json::Value::Array(Vec::new());
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&out) {
        if val.is_array() {
            return val;
        } else if val.is_object() {
            return serde_json::Value::Array(vec![val]);
        }
    }
    serde_json::Value::Array(Vec::new())
}

pub(crate) fn parse_volume(vol_obj: &serde_json::Value) -> i64 {
    if let Some(obj) = vol_obj.as_object() {
        for (_chan, val_dict) in obj {
            if let Some(val_str) = val_dict.get("value_percent").and_then(|v| v.as_str())
                && let Some(vol) = parse_percent(val_str)
            {
                return vol;
            }
        }
    }
    0
}

pub(crate) fn sample_rate_of(sample_spec: &str) -> String {
    if let Some(hz_pos) = sample_spec.find("Hz") {
        let prefix = &sample_spec[..hz_pos];
        let digits: String = prefix
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let digits: String = digits.chars().rev().collect();
        if let Ok(hz) = digits.parse::<i64>() {
            return format!("{}kHz", hz / 1000);
        }
    }
    "48kHz".to_string()
}

pub(crate) fn sink_icon(is_bt: bool, active_port: &str) -> &'static str {
    if is_bt {
        "audio-headphones-bluetooth"
    } else if active_port.contains("headphone") {
        "audio-headphones"
    } else {
        "audio-speakers"
    }
}

pub(crate) fn parse_pactl_info(out: &str) -> (String, String, String) {
    let mut default_sink_name = String::new();
    let mut default_source_name = String::new();
    let mut pipewire_version = "Running".to_string();
    for line in out.lines() {
        if let Some(suffix) = line.strip_prefix("Default Sink:") {
            default_sink_name = suffix.trim().to_string();
        } else if let Some(suffix) = line.strip_prefix("Default Source:") {
            default_source_name = suffix.trim().to_string();
        } else if let Some(suffix) = line.strip_prefix("Server Name:") {
            let server_name = suffix.trim();
            if let Some(ver) = server_name.find('(').and_then(|start| {
                server_name.find(')').and_then(|end| {
                    if end > start {
                        Some(server_name[start + 1..end].to_string())
                    } else {
                        None
                    }
                })
            }) {
                pipewire_version = ver;
            }
        }
    }
    (default_sink_name, default_source_name, pipewire_version)
}
