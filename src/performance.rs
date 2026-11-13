//! Performance-tune: apply/revert the extras for the `performance` power
//! profile (GPU DPM, PCIe ASPM, vm dirty writeback). Native port of
//! anixie's `performance.sh`.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
// thread not used
// (no duration needed) 
// remove


fn env_str(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn state_dir() -> PathBuf {
    PathBuf::from(env_str("STATE", "/run/anixie/performance"))
}

fn ppd() -> String {
    env_str("PPD", "powerprofilesctl")
}
fn busctl() -> String {
    env_str("BUSCTL", "busctl")
}
fn sysctl() -> String {
    env_str("SYSCTL", "sysctl")
}

fn gpu_level() -> Option<PathBuf> {
    let mut fallback: Option<PathBuf> = None;
    if let Ok(rd) = fs::read_dir("/sys/class/drm") {
        for e in rd.flatten() {
            let f = e.path().join("device/power_dpm_force_performance_level");
            if !f.exists() {
                continue;
            }
            let boot_vga = fs::read_to_string(e.path().join("device/boot_vga"))
                .unwrap_or_default();
            if boot_vga.trim() == "1" {
                return Some(f);
            }
            if fallback.is_none() {
                fallback = Some(f);
            }
        }
    }
    fallback
}

fn aspm_policy() -> Option<String> {
    let p = "/sys/module/pcie_aspm/parameters/policy";
    let content = fs::read_to_string(p).ok()?;
    // extract bracketed value
    let start = content.find('[')?;
    let rest = &content[start + 1..];
    let end = rest.find(']')?;
    Some(rest[..end].to_string())
}

fn sysctl_read(key: &str) -> String {
    let out = Command::new(sysctl()).args(["-n", key]).output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

fn sysctl_write(pairs: &[(&str, &str)]) {
    let mut args: Vec<String> = vec!["-q".to_string(), "-w".to_string()];
    for (k, v) in pairs {
        args.push(format!("{}={}", k, v));
    }
    let _ = Command::new(sysctl()).args(&args).status();
}

fn remember(key: &str, value: &str) {
    let dir = state_dir();
    let _ = fs::create_dir_all(&dir);
    let p = dir.join(key);
    if !p.exists() {
        let _ = fs::write(&p, format!("{}\n", value));
    }
}

fn set_value(path: &Path, value: &str) {
    if fs::write(path, format!("{}\n", value)).is_err() {
        eprintln!("performance-tune: cannot set {}", path.display());
    }
}

fn performance_on() {
    let dir = state_dir();
    let _ = fs::create_dir_all(&dir);

    if let Some(gpu) = gpu_level() {
        let cur = fs::read_to_string(&gpu).unwrap_or_default();
        remember("gpu", cur.trim());
        set_value(&gpu, "high");
    }

    if let Some(policy) = aspm_policy() {
        remember("aspm", &policy);
        let _ = fs::write("/sys/module/pcie_aspm/parameters/policy", "performance\n");
    }

    for key in ["vm.dirty_background_ratio", "vm.dirty_ratio", "vm.dirty_background_bytes", "vm.dirty_bytes"] {
        let v = sysctl_read(key);
        remember(key, &v);
    }

    let family_file = dir.join("dirty_family");
    if !family_file.exists() {
        let bg_bytes = fs::read_to_string(dir.join("vm.dirty_background_bytes"))
            .unwrap_or_default();
        let bytes = fs::read_to_string(dir.join("vm.dirty_bytes")).unwrap_or_default();
        let family = if bg_bytes.trim() != "0" || bytes.trim() != "0" {
            "bytes"
        } else {
            "ratio"
        };
        let _ = fs::write(&family_file, format!("{}\n", family));
    }

    sysctl_write(&[
        ("vm.dirty_background_bytes", "134217728"),
        ("vm.dirty_bytes", "536870912"),
    ]);
}

fn performance_off() {
    let dir = state_dir();
    if !dir.exists() {
        return;
    }

    if let Some(gpu) = gpu_level()
        && let Ok(v) = fs::read_to_string(dir.join("gpu"))
    {
        set_value(&gpu, v.trim());
    }

    if let Ok(v) = fs::read_to_string(dir.join("aspm")) {
        let _ = fs::write("/sys/module/pcie_aspm/parameters/policy", format!("{}\n", v.trim()));
    }

    if let Ok(family) = fs::read_to_string(dir.join("dirty_family")) {
        match family.trim() {
            "bytes" => {
                let bg = fs::read_to_string(dir.join("vm.dirty_background_bytes")).unwrap_or_default();
                let bytes = fs::read_to_string(dir.join("vm.dirty_bytes")).unwrap_or_default();
                sysctl_write(&[
                    ("vm.dirty_background_bytes", bg.trim()),
                    ("vm.dirty_bytes", bytes.trim()),
                ]);
            }
            _ => {
                let bg = fs::read_to_string(dir.join("vm.dirty_background_ratio")).unwrap_or_default();
                let ratio = fs::read_to_string(dir.join("vm.dirty_ratio")).unwrap_or_default();
                sysctl_write(&[
                    ("vm.dirty_background_ratio", bg.trim()),
                    ("vm.dirty_ratio", ratio.trim()),
                ]);
            }
        }
    }

    let _ = fs::remove_dir_all(&dir);
}

fn current_profile() -> String {
    let out = Command::new(ppd()).arg("get").output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

fn apply() {
    match current_profile().as_str() {
        "performance" => performance_on(),
        _ => performance_off(),
    }
}

fn extras_active() -> bool {
    if let Some(gpu) = gpu_level()
        && let Ok(v) = fs::read_to_string(&gpu)
        && v.trim() == "high"
    {
        return true;
    }
    if let Some(p) = aspm_policy()
        && p == "performance"
    {
        return true;
    }
    if sysctl_read("vm.dirty_background_bytes") == "134217728"
        && sysctl_read("vm.dirty_bytes") == "536870912"
    {
        return true;
    }
    false
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "-h" | "--help" | "help" => {
            "usage: performance [status|apply|watch]\n\n  status  whether the extras are in effect\n  apply   apply or revert for the current profile\n  watch   apply now, then on every profile change".to_string()
        }
        "" | "status" => {
            if extras_active() {
                "performance extras: on".to_string()
            } else {
                "performance extras: off".to_string()
            }
        }
        "apply" => {
            apply();
            "OK".to_string()
        }
        "watch" => {
            apply();
            let busctl_bin = busctl();
            let match_arg = "type='signal',interface='org.freedesktop.DBus.Properties',path='/net/hadess/PowerProfiles',member='PropertiesChanged'";
            let mut child = match Command::new(&busctl_bin)
                .args(["monitor", "--system", "--match", match_arg])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => return format!("ERR {}", e),
            };
            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if line.contains("Path=/net/hadess/PowerProfiles") {
                        apply();
                    }
                }
            }
            let _ = child.wait();
            "OK".to_string()
        }
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_active_does_not_panic() {
        let _ = extras_active();
    }

    #[test]
    fn handle_help() {
        let out = handle(&["--help".to_string()]);
        assert!(out.contains("usage: performance"));
    }

    #[test]
    fn split_name_not_needed() {
        // placeholder to keep module tested
    }
}
