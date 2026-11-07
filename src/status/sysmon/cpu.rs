//! CPU: /proc/stat, cpuinfo, thermal, freq, power estimate.

use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::thread::sleep;
use std::time::Duration;
pub(crate) fn parse_cpu_times_line(line: &str) -> Option<(u64, u64)> {
    if !line.starts_with("cpu ") {
        return None;
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 9 {
        return None;
    }
    let user: u64 = parts[1].parse().unwrap_or(0);
    let nice: u64 = parts[2].parse().unwrap_or(0);
    let system: u64 = parts[3].parse().unwrap_or(0);
    let idle: u64 = parts[4].parse().unwrap_or(0);
    let iowait: u64 = parts[5].parse().unwrap_or(0);
    let irq: u64 = parts[6].parse().unwrap_or(0);
    let softirq: u64 = parts[7].parse().unwrap_or(0);
    let steal: u64 = parts[8].parse().unwrap_or(0);
    let total = user + nice + system + idle + iowait + irq + softirq + steal;
    let idle_total = idle + iowait;
    Some((total, idle_total))
}

fn read_cpu_times() -> Option<(u64, u64)> {
    let file = File::open("/proc/stat").ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.ok()?;
        if let Some(v) = parse_cpu_times_line(&line) {
            return Some(v);
        }
    }
    None
}

pub(crate) fn clean_cpu_name(model: &str) -> String {
    if model.contains("i7-12700H") {
        return "i7-12700H".to_string();
    }
    model
        .replace("Intel(R) Core(TM)", "")
        .replace("12th Gen", "")
        .replace("AMD", "")
        .replace("Ryzen", "")
        .replace("Processor", "")
        .trim()
        .to_string()
}

pub(crate) fn get_cpu_name() -> String {
    if let Ok(file) = File::open("/proc/cpuinfo") {
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            if line.starts_with("model name")
                && let Some(pos) = line.find(':')
            {
                let model = line[pos + 1..].trim();
                let cleaned = clean_cpu_name(model);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
    }
    "CPU".to_string()
}

/// CPU usage from two samples.
pub(crate) fn cpu_usage_from(total_idle: ((u64, u64), (u64, u64))) -> i32 {
    let ((total1, idle1), (total2, idle2)) = total_idle;
    let total_diff = total2.saturating_sub(total1);
    let idle_diff = idle2.saturating_sub(idle1);
    if total_diff == 0 {
        return 0;
    }
    let usage = 100.0 * (total_diff as f64 - idle_diff as f64) / total_diff as f64;
    usage.round() as i32
}

pub(crate) fn get_cpu_usage() -> i32 {
    if let Some(a) = read_cpu_times() {
        sleep(Duration::from_millis(150));
        if let Some(b) = read_cpu_times() {
            return cpu_usage_from((a, b));
        }
    }
    0
}

pub(crate) fn get_cpu_temp() -> i32 {
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_zone = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("thermal_zone"));
            if !is_zone {
                continue;
            }
            let type_path = path.join("type");
            let temp_path = path.join("temp");
            if let Ok(type_str) = fs::read_to_string(type_path) {
                let type_str = type_str.trim();
                if (type_str == "x86_pkg_temp" || type_str == "TCPU")
                    && let Ok(temp_str) = fs::read_to_string(temp_path)
                        && let Ok(temp_raw) = temp_str.trim().parse::<i32>()
                    {
                        return temp_raw / 1000;
                    }
            }
        }
    }
    if let Ok(temp_str) = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        && let Ok(temp_raw) = temp_str.trim().parse::<i32>()
    {
        return temp_raw / 1000;
    }
    0
}

pub(crate) fn parse_cpu_freqs(content: &str) -> Vec<f64> {
    let mut out = Vec::new();
    for line in content.lines() {
        if line.starts_with("cpu MHz")
            && let Some(pos) = line.find(':')
            && let Ok(mhz) = line[pos + 1..].trim().parse::<f64>()
        {
            out.push(mhz);
        }
    }
    out
}

pub(crate) fn get_cpu_freq() -> f64 {
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        let freqs = parse_cpu_freqs(&content);
        if !freqs.is_empty() {
            return (freqs.iter().sum::<f64>() / freqs.len() as f64) / 1000.0;
        }
    }
    0.0
}

pub(crate) fn cpu_power_estimate(usage: i32) -> f64 {
    4.5 + (usage as f64 / 100.0) * 40.5
}
