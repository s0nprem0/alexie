//! System monitor: CPU/GPU/RAM/disks.

use super::common::{print_json, read_trimmed, run_out, nvidia_smi, inxi, lsblk, cache_dir};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub read_rate: f64,
    pub write_rate: f64,
    pub total_gb: f64,
    pub used_gb: f64,
    pub free_gb: f64,
    pub usage_pct: i32,
}

impl Default for DiskInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            read_rate: 0.0,
            write_rate: 0.0,
            total_gb: 0.0,
            used_gb: 0.0,
            free_gb: 0.0,
            usage_pct: 0,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct SysmonStatus {
    pub cpu_name: String,
    pub cpu_usage: i32,
    pub cpu_temp: i32,
    pub cpu_freq: f64,
    pub cpu_power: f64,
    pub gpu_name: String,
    pub gpu_usage: i32,
    pub gpu_temp: i32,
    pub gpu_power: f64,
    pub gpu_mem_used: i32,
    pub gpu_mem_total: i32,
    pub ram_name: String,
    pub ram_speed: String,
    pub ram_total: f64,
    pub ram_used: f64,
    pub disks: Vec<DiskInfo>,
    pub disk0: DiskInfo,
    pub disk1: DiskInfo,
}

/// Parse a `/proc/stat` cpu line.
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

fn get_cpu_name() -> String {
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

fn get_cpu_usage() -> i32 {
    if let Some(a) = read_cpu_times() {
        sleep(Duration::from_millis(150));
        if let Some(b) = read_cpu_times() {
            return cpu_usage_from((a, b));
        }
    }
    0
}

fn get_cpu_temp() -> i32 {
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
                if type_str == "x86_pkg_temp" || type_str == "TCPU" {
                    if let Ok(temp_str) = fs::read_to_string(temp_path)
                        && let Ok(temp_raw) = temp_str.trim().parse::<i32>()
                    {
                        return temp_raw / 1000;
                    }
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

fn get_cpu_freq() -> f64 {
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

fn get_gpu_name() -> String {
    let out = run_out(&nvidia_smi(), &["--query-gpu=name", "--format=csv,noheader"]);
    let name = out.trim();
    if name.is_empty() {
        return String::new();
    }
    if name.contains("RTX 3060") {
        return "RTX 3060".to_string();
    }
    name.replace("NVIDIA GeForce", "")
        .replace("Laptop GPU", "")
        .replace("GPU", "")
        .trim()
        .to_string()
}

fn get_gpu_usage() -> i32 {
    let out = run_out(&nvidia_smi(), &["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"]);
    out.trim().parse::<i32>().unwrap_or(0)
}

fn get_gpu_temp() -> i32 {
    let out = run_out(&nvidia_smi(), &["--query-gpu=temperature.gpu", "--format=csv,noheader,nounits"]);
    out.trim().parse::<i32>().unwrap_or(0)
}

fn get_gpu_power() -> f64 {
    let out = run_out(&nvidia_smi(), &["--query-gpu=power.draw", "--format=csv,noheader,nounits"]);
    out.trim().parse::<f64>().unwrap_or(0.0)
}

fn get_gpu_memory() -> (i32, i32) {
    let out = run_out(&nvidia_smi(), &["--query-gpu=memory.used,memory.total", "--format=csv,noheader,nounits"]);
    let parts: Vec<&str> = out.split(',').collect();
    if parts.len() == 2 {
        let used = parts[0].trim().parse::<i32>().unwrap_or(0);
        let total = parts[1].trim().parse::<i32>().unwrap_or(0);
        return (used, total);
    }
    (0, 0)
}

pub(crate) fn parse_mem_line(line: &str) -> f64 {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].parse::<f64>().unwrap_or(0.0) / 1024.0 / 1024.0
    } else {
        0.0
    }
}

fn get_ram_usage_info() -> (f64, f64) {
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

fn get_static_ram_info() -> (String, String) {
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
    let output = run_out(&lsblk(), &["-lnb", "-o", "FSSIZE,FSUSED", &format!("/dev/{dev}")]);

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

fn get_disk_info(dev: &str) -> DiskInfo {
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

pub(crate) fn collect() -> SysmonStatus {
    let cpu_n = get_cpu_name();
    let cpu = get_cpu_usage();
    let cpu_t = get_cpu_temp();
    let cpu_f = get_cpu_freq();
    let cpu_p = cpu_power_estimate(cpu);
    let gpu_n = get_gpu_name();
    let gpu = get_gpu_usage();
    let gpu_t = get_gpu_temp();
    let gpu_p = get_gpu_power();
    let (gpu_mem_u, gpu_mem_t) = get_gpu_memory();
    let (ram_tot, ram_usd) = get_ram_usage_info();
    let (ram_name, ram_speed) = get_static_ram_info();

    let devs = discover_disks();
    let disks: Vec<DiskInfo> = devs.iter().map(|d| get_disk_info(d)).collect();
    let disk0 = disks.first().cloned().unwrap_or_default();
    let disk1 = disks.get(1).cloned().unwrap_or_default();

    SysmonStatus {
        cpu_name: cpu_n,
        cpu_usage: cpu,
        cpu_temp: cpu_t,
        cpu_freq: cpu_f,
        cpu_power: cpu_p,
        gpu_name: gpu_n,
        gpu_usage: gpu,
        gpu_temp: gpu_t,
        gpu_power: gpu_p,
        gpu_mem_used: gpu_mem_u,
        gpu_mem_total: gpu_mem_t,
        ram_name,
        ram_speed,
        ram_total: ram_tot,
        ram_used: ram_usd,
        disks,
        disk0,
        disk1,
    }
}

pub(crate) fn render_text(s: &SysmonStatus) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "cpu {}  usage {}%  temp {}C  freq {:.2}GHz  power {:.1}W\n",
        s.cpu_name, s.cpu_usage, s.cpu_temp, s.cpu_freq, s.cpu_power
    ));
    if !s.gpu_name.is_empty() {
        out.push_str(&format!(
            "gpu {}  usage {}%  temp {}C  power {:.1}W  mem {}/{}MiB\n",
            s.gpu_name, s.gpu_usage, s.gpu_temp, s.gpu_power, s.gpu_mem_used, s.gpu_mem_total
        ));
    }
    out.push_str(&format!(
        "ram {} {}  total {:.1}GiB  used {:.1}GiB  ({}%)\n",
        s.ram_name,
        s.ram_speed,
        s.ram_total,
        s.ram_used,
        if s.ram_total > 0.0 { (s.ram_used / s.ram_total * 100.0).round() as i32 } else { 0 }
    ));
    if !s.disks.is_empty() {
        let rows: Vec<Vec<String>> = s
            .disks
            .iter()
            .map(|d| {
                vec![
                    d.name.clone(),
                    format!("{:.1}MiB/s", d.read_rate),
                    format!("{:.1}MiB/s", d.write_rate),
                    format!("{:.1}GiB", d.total_gb),
                    format!("{:.1}GiB", d.used_gb),
                    format!("{}%", d.usage_pct),
                ]
            })
            .collect();
        out.push_str(&crate::table::render(&["Disk", "Read", "Write", "Total", "Used", "Usage"], &rows));
    }
    out.trim_end().to_string()
}

pub(crate) fn handle(args: &[String]) -> String {
    let want_json = args.iter().any(|a| a == "--json");
    for a in args {
        if a.starts_with("--") && a != "--json" {
            return format!("ERR unknown flag: {}", a);
        }
    }
    let status = collect();
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
    fn cpu_line_parses() {
        let line = "cpu  100 0 100 800 50 0 0 0 0 0";
        let (total, idle) = parse_cpu_times_line(line).unwrap();
        assert_eq!(total, 1050);
        assert_eq!(idle, 850);
        assert_eq!(cpu_usage_from(((1000, 800), (1050, 850))), 0);
    }

    #[test]
    fn cpu_usage_math() {
        // 50% busy: total diff 100, idle diff 50
        assert_eq!(cpu_usage_from(((1000, 500), (1100, 550))), 50);
        assert_eq!(cpu_usage_from(((1000, 500), (1000, 500))), 0);
    }

    #[test]
    fn mem_line_parses() {
        assert_eq!(parse_mem_line("MemTotal:        16777216 kB"), 16.0);
        assert_eq!(parse_mem_line("junk"), 0.0);
    }

    #[test]
    fn cpu_freq_parses() {
        let content = "cpu MHz\t\t: 2400.000\ncpu MHz\t\t: 2600.000\nmodel name\t: foo";
        assert_eq!(parse_cpu_freqs(content), vec![2400.0, 2600.0]);
    }

    #[test]
    fn disk_stat_parses() {
        // /sys/block/<dev>/stat: fields 3/7 are sector counts (index 2/6).
        let content = "8 0 100 0 50 0 200 0 60 0 60 0";
        let (r, w) = parse_disk_stat(content).unwrap();
        assert_eq!(r, 100);
        assert_eq!(w, 200);
    }

    #[test]
    fn cpu_name_cleaning() {
        assert_eq!(clean_cpu_name("Intel(R) Core(TM) i7-12700H"), "i7-12700H");
        assert!(clean_cpu_name("AMD Ryzen 5 Processor").contains("5"));
    }

    #[test]
    fn handle_json_is_parseable() {
        let out = handle(&["--json".to_string()]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("sysmon --json must be JSON");
        assert!(v.get("cpu_usage").is_some());
        assert!(v.get("ram_total").is_some());
        assert!(v.get("disks").is_some());
    }

    #[test]
    fn handle_unknown_flag_fails_fast() {
        let out = handle(&["--bogus".to_string()]);
        assert!(out.starts_with("ERR unknown flag:"));
    }
}
