// Power watcher: udev monitor + 30s timer. Replicates powerprofile.sh watch.
// Delegates the heavy battery logic to power::check_battery / power::apply via
// the existing functions (re-implemented here as calls into power module).
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub fn run() {
    // apply once at login
    crate::power::apply_pub();
    let _state_dir = crate::power::state_dir_pub();

    // udev monitor thread
    let _monitor_handle = thread::spawn(move || {
        let child = Command::new("udevadm")
            .args(["monitor", "--subsystem-match=power_supply", "--property"])
            .stdout(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(_) => return,
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.wait();
            return;
        };
        let reader = BufReader::new(stdout);
        let mut last_online: Option<i32> = None;
        let mut event_has_online = false;
        let mut event_online: Option<i32> = None;
        let mut event_status = String::new();
        let mut event_capacity = String::new();
        let mut last_sig = String::new();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.starts_with("POWER_SUPPLY_ONLINE=") {
                event_has_online = true;
                event_online = line.trim_start_matches("POWER_SUPPLY_ONLINE=").parse().ok();
            } else if line.starts_with("POWER_SUPPLY_STATUS=") {
                event_status = line.trim_start_matches("POWER_SUPPLY_STATUS=").to_string();
            } else if line.starts_with("POWER_SUPPLY_CAPACITY=") {
                event_capacity = line
                    .trim_start_matches("POWER_SUPPLY_CAPACITY=")
                    .to_string();
            } else if line.is_empty() {
                let sig = format!(
                    "{}:{}|{}|{}",
                    if event_has_online { 1 } else { 0 },
                    event_online.map(|v| v.to_string()).unwrap_or_default(),
                    event_status,
                    event_capacity
                );
                if !event_has_online
                    && event_status.is_empty()
                    && event_capacity.is_empty()
                    && event_online.is_none()
                {
                    // nothing
                } else if sig != last_sig {
                    last_sig = sig.clone();
                    if event_has_online {
                        if let Some(o) = event_online {
                            if last_online != Some(o) {
                                last_online = Some(o);
                                crate::power::apply_pub();
                            } else {
                                crate::power::check_battery_pub();
                            }
                        } else {
                            crate::power::check_battery_pub();
                        }
                    } else {
                        crate::power::check_battery_pub();
                    }
                }
                event_has_online = false;
                event_online = None;
                event_status.clear();
                event_capacity.clear();
            }
        }
        let _ = child.wait();
    });

    // 30s timer
    loop {
        thread::sleep(Duration::from_secs(30));
        crate::power::check_battery_pub();
    }
}
