//! Disks: /sys/block io rates + lsblk usage.

use super::super::common::{cache_dir, lsblk, read_trimmed, run_out};
use super::types::DiskInfo;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

const SECTOR_SIZE: u64 = 512;

fn stat_dir() -> std::path::PathBuf {
    cache_dir().join("disk_stats")
}

pub(crate) fn parse_disk_stat(content: &str) -> Option<(u64, u64)> {
    let fields: Vec<&str> = content.split_whitespace().collect();
    if fields.len() >= 7 {
        let sectors_read = fields[2].parse::<u64>().unwrap_or(0);
        let sectors_written = fields[6].parse::<u64>().unwrap_or(0);
        Some((sectors_read, sectors_written))
    } else {
        None
    }
}

fn read_disk_sectors(dev: &str) -> Option<(u64, u64)> {
    let stat_path = format!("/sys/block/{dev}/stat");
    let content = fs::read_to_string(stat_path).ok()?;
    parse_disk_stat(&content)
}

fn disk_io_rates(dev: &str) -> (f64, f64) {
    let dir = stat_dir();
    let _ = fs::create_dir_all(&dir);
    let prev_file = dir.join(dev);
    let (sectors_read, sectors_written) = read_disk_sectors(dev).unwrap_or((0, 0));
    let now = now_nanos();

    let mut read_rate = 0.0;
    let mut write_rate = 0.0;

    if let Ok(content) = fs::read_to_string(&prev_file) {
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() >= 3 {
            let prev_time: u64 = lines[0].parse().unwrap_or(0);
            let prev_read: u64 = lines[1].parse().unwrap_or(0);
            let prev_write: u64 = lines[2].parse().unwrap_or(0);
            let dt_ns = now.saturating_sub(prev_time);
            if dt_ns > 100_000_000 {
                let dt_s = dt_ns as f64 / 1_000_000_000.0;
                let dr = sectors_read.saturating_sub(prev_read);
                let dw = sectors_written.saturating_sub(prev_write);
                read_rate = (dr as f64 * SECTOR_SIZE as f64) / (1024.0 * 1024.0) / dt_s;
                write_rate = (dw as f64 * SECTOR_SIZE as f64) / (1024.0 * 1024.0) / dt_s;
            }
        }
    }

    let snapshot = format!("{now}\n{sectors_read}\n{sectors_written}\n");
    let _ = fs::write(&prev_file, snapshot);

    (read_rate, write_rate)
}

fn disk_usage(dev: &str) -> (f64, f64, f64, i32) {
    let output = run_out(
        &lsblk(),
        &["-lnb", "-o", "FSSIZE,FSUSED", &format!("/dev/{dev}")],
    );

    let mut agg_total: u64 = 0;
    let mut agg_used: u64 = 0;
    let mut seen_totals: Vec<u64> = Vec::new();

    for line in output.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 2 {
            let fs_size: u64 = fields[0].parse().unwrap_or(0);
            let fs_used: u64 = fields[1].parse().unwrap_or(0);
            if fs_size == 0 {
                continue;
            }
            if seen_totals.contains(&fs_size) {
                continue;
            }
            seen_totals.push(fs_size);
            agg_total += fs_size;
            agg_used += fs_used;
        }
    }

    let gb = 1024.0 * 1024.0 * 1024.0;
    if agg_total > 0 {
        let total_gb = agg_total as f64 / gb;
        let used_gb = agg_used as f64 / gb;
        let free_gb = (agg_total.saturating_sub(agg_used)) as f64 / gb;
        let usage_pct = ((agg_used as f64 / agg_total as f64) * 100.0).round() as i32;
        (total_gb, used_gb, free_gb, usage_pct)
    } else {
        let raw = read_trimmed(Path::new(&format!("/sys/block/{dev}/size")))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let total_gb = (raw * SECTOR_SIZE) as f64 / gb;
        (total_gb, 0.0, total_gb, 0)
    }
}

fn get_disk_name(dev: &str) -> String {
    read_trimmed(Path::new(&format!("/sys/block/{dev}/device/model")))
        .unwrap_or_else(|| dev.to_string())
}

pub(crate) fn get_disk_info(dev: &str) -> DiskInfo {
    let name = get_disk_name(dev);
    let (read_rate, write_rate) = disk_io_rates(dev);
    let (total_gb, used_gb, free_gb, usage_pct) = disk_usage(dev);
    DiskInfo {
        name,
        read_rate: (read_rate * 10.0).round() / 10.0,
        write_rate: (write_rate * 10.0).round() / 10.0,
        total_gb: (total_gb * 10.0).round() / 10.0,
        used_gb: (used_gb * 10.0).round() / 10.0,
        free_gb: (free_gb * 10.0).round() / 10.0,
        usage_pct,
    }
}

pub(crate) fn discover_disks() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/block") {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_disk = name.starts_with("nvme")
                || name.starts_with("sd")
                || name.starts_with("vd")
                || name.starts_with("hd")
                || name.starts_with("mmcblk");
            // Skip partition-style digits-only virtual devices (e.g. dm-*, loop* stay excluded).
            if is_disk && !name.contains('-') {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}
