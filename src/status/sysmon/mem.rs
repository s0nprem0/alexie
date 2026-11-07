//! RAM: /proc/meminfo + inxi.

use super::super::common::{inxi, run_out};
use std::fs::File;
use std::io::{BufRead, BufReader};
pub(crate) fn parse_mem_line(line: &str) -> f64 {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].parse::<f64>().unwrap_or(0.0) / 1024.0 / 1024.0
    } else {
        0.0
    }
}

pub(crate) fn get_ram_usage_info() -> (f64, f64) {
    let mut total = 0.0;
    let mut available = 0.0;
    if let Ok(file) = File::open("/proc/meminfo") {
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            if line.starts_with("MemTotal:") {
                total = parse_mem_line(&line);
            } else if line.starts_with("MemAvailable:") {
                available = parse_mem_line(&line);
            }
        }
    }
    let used = (total - available).max(0.0);
    (total, used)
}

pub(crate) fn get_static_ram_info() -> (String, String) {
    let out = run_out(&inxi(), &["-m", "-c", "0"]);
    let mut ram_type = "DDR".to_string();
    let mut speed = "N/A".to_string();
    for line in out.lines() {
        if line.contains("type:")
            && let Some(pos) = line.find("type:")
        {
            let rest = &line[pos + 5..];
            if let Some(space_pos) = rest.trim().find(' ') {
                ram_type = rest.trim()[..space_pos].trim().to_string();
            } else {
                ram_type = rest.trim().to_string();
            }
        }
        if line.contains("speed:")
            && let Some(pos) = line.find("speed:")
        {
            speed = line[pos + 6..].trim().to_string();
        }
    }
    (ram_type, speed)
}
