//! System monitor: CPU/GPU/RAM/disks.
//!
//! Split from a single 597-line `sysmon.rs`:
//! - `types` — DiskInfo/SysmonStatus
//! - `cpu` — /proc/stat, cpuinfo, thermal, freq, power
//! - `gpu` — nvidia-smi GPU queries
//! - `mem` — /proc/meminfo + inxi
//! - `disk` — /sys/block io rates + lsblk usage
//!
//! `collect()`/`render_text()`/`handle()` below just orchestrate.

pub(crate) mod cpu;
pub(crate) mod disk;
pub(crate) mod gpu;
pub(crate) mod mem;
pub(crate) mod types;

#[allow(unused_imports)] // re-exported for public API compat
pub use types::{DiskInfo, SysmonStatus};

use super::common::print_json;
#[cfg(test)]
use cpu::{clean_cpu_name, cpu_usage_from, parse_cpu_freqs, parse_cpu_times_line};
use cpu::{cpu_power_estimate, get_cpu_freq, get_cpu_name, get_cpu_temp, get_cpu_usage};
#[cfg(test)]
use disk::parse_disk_stat;
use disk::{discover_disks, get_disk_info};
use gpu::{get_gpu_memory, get_gpu_name, get_gpu_power, get_gpu_temp, get_gpu_usage};
#[cfg(test)]
use mem::parse_mem_line;
use mem::{get_ram_usage_info, get_static_ram_info};

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
        if s.ram_total > 0.0 {
            (s.ram_used / s.ram_total * 100.0).round() as i32
        } else {
            0
        }
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
        out.push_str(&crate::table::render(
            &["Disk", "Read", "Write", "Total", "Used", "Usage"],
            &rows,
        ));
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
