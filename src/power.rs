use std::env;
use std::fs;
use std::process::Command;
use std::path::PathBuf;

fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

// Placeholders would be rendered by Nix; here we use env overrides with defaults.
fn ppd() -> String { env::var("PPD").unwrap_or_else(|_| "powerprofilesctl".to_string()) }
fn brightnessctl() -> String { env::var("BRIGHTNESSCTL").unwrap_or_else(|_| "brightnessctl".to_string()) }
fn charge_limit_bin() -> String { env::var("CHARGE_LIMIT").unwrap_or_else(|_| "charge-limit".to_string()) }
fn sudo() -> String { env::var("SUDO").unwrap_or_else(|_| "sudo".to_string()) }
fn ac_profile_default() -> String { env::var("AC_PROFILE").unwrap_or_else(|_| "performance".to_string()) }
fn battery_profile_default() -> String { env::var("BATTERY_PROFILE").unwrap_or_else(|_| "power-saver".to_string()) }
fn low_profile() -> String { env::var("LOW_PROFILE").unwrap_or_else(|_| "balanced".to_string()) }
fn critical_profile() -> String { env::var("CRITICAL_PROFILE").unwrap_or_else(|_| "power-saver".to_string()) }
fn low_pct() -> i64 { env::var("LOW").ok().and_then(|s| s.parse().ok()).unwrap_or(20) }
fn critical_pct() -> i64 { env::var("CRITICAL").ok().and_then(|s| s.parse().ok()).unwrap_or(8) }
fn low_brightness() -> i64 { env::var("LOW_BRIGHTNESS").ok().and_then(|s| s.parse().ok()).unwrap_or(30) }
fn critical_brightness() -> i64 { env::var("CRITICAL_BRIGHTNESS").ok().and_then(|s| s.parse().ok()).unwrap_or(10) }
fn auto_default() -> bool { env::var("AUTO").map(|s| s == "1").unwrap_or(true) }

fn on_mains() -> bool {
    // check /sys/class/power_supply/*/online
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let p = e.path().join("online");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if s.trim() == "1" { return true; }
            }
        }
    }
    false
}

fn source_name() -> &'static str { if on_mains() { "ac" } else { "battery" } }

fn battery_percent() -> Option<i64> {
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("BAT") { continue; }
            let p = e.path().join("capacity");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(v) = s.trim().parse::<i64>() { return Some(v); }
            }
        }
    }
    None
}

fn charge_ceiling() -> Option<i64> {
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for e in entries.flatten() {
            let p = e.path().join("charge_control_end_threshold");
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(v) = s.trim().parse::<i64>() { return Some(v); }
            }
        }
    }
    None
}

fn automation_file() -> PathBuf { state_dir().join("automation") }
fn state_file_src(src: &str) -> PathBuf { state_dir().join(src) }
fn saved_brightness_file() -> PathBuf { state_dir().join("brightness.saved") }
fn dim_level_file() -> PathBuf { state_dir().join("brightness.level") }
fn last_source_file() -> PathBuf { state_dir().join("last-source") }

fn automation_enabled() -> bool {
    let p = automation_file();
    if p.exists() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            match s.trim() {
                "on" => return true,
                "off" => return false,
                _ => {}
            }
        }
    }
    auto_default()
}

fn preferred(src: &str) -> String {
    let fallback = if src == "ac" { ac_profile_default() } else { battery_profile_default() };
    let p = state_file_src(src);
    if p.exists() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            let s = s.trim().to_string();
            if !s.is_empty() { return s; }
        }
    }
    fallback
}

fn backlight_percent() -> Option<i64> {
    let (code, cur, _) = run(&brightnessctl(), &["-c", "backlight", "get"]);
    if code != 0 { return None; }
    let (code2, max, _) = run(&brightnessctl(), &["-c", "backlight", "max"]);
    if code2 != 0 { return None; }
    let cur: i64 = cur.trim().parse().ok()?;
    let max: i64 = max.trim().parse().ok()?;
    if max <= 0 { return None; }
    Some(cur * 100 / max)
}

fn set_backlight(pct: i64) {
    let _ = run(&brightnessctl(), &["-c", "backlight", "set", &format!("{}%", pct)]);
}

fn restore_brightness() {
    if let Ok(s) = std::fs::read_to_string(saved_brightness_file()) {
        if let Ok(v) = s.trim().parse::<i64>() {
            set_backlight(v);
        }
    }
    let _ = std::fs::remove_file(saved_brightness_file());
    let _ = std::fs::remove_file(dim_level_file());
}

fn enter_dim(level: &str, target: i64) {
    if let Ok(s) = std::fs::read_to_string(dim_level_file()) {
        if s.trim() == level { return; }
    }
    if !saved_brightness_file().exists() {
        if let Some(cur) = backlight_percent() {
            let _ = std::fs::write(saved_brightness_file(), format!("{}\n", cur));
        }
    }
    set_backlight(target);
    let _ = std::fs::write(dim_level_file(), format!("{}\n", level));
}

fn apply() {
    let _ = std::fs::create_dir_all(state_dir());
    let src = source_name();
    let mut profile = preferred(src);

    if src == "ac" {
        restore_brightness();
    } else if automation_enabled() {
        if let Some(pct) = battery_percent() {
            if pct <= critical_pct() {
                profile = critical_profile();
                enter_dim("critical", critical_brightness());
            } else if pct <= low_pct() {
                profile = low_profile();
                enter_dim("low", low_brightness());
            } else {
                restore_brightness();
            }
        }
    }

    let ppd_bin = ppd();
    let _ = run(&ppd_bin, &["set", &profile]);
    let _ = std::fs::write(last_source_file(), format!("{}\n", src));
}

fn check_battery() {
    if !automation_enabled() { return; }
    let _ = std::fs::create_dir_all(state_dir());
    let src = source_name();
    let last = std::fs::read_to_string(last_source_file()).unwrap_or_default();
    if last.trim() != src {
        apply();
        return;
    }
    if src == "ac" {
        restore_brightness();
        return;
    }
    if let Some(pct) = battery_percent() {
        let ppd_bin = ppd();
        if pct <= critical_pct() {
            let _ = run(&ppd_bin, &["set", &critical_profile()]);
            enter_dim("critical", critical_brightness());
        } else if pct <= low_pct() {
            let _ = run(&ppd_bin, &["set", &low_profile()]);
            enter_dim("low", low_brightness());
        } else {
            restore_brightness();
        }
    }
}

fn set_pref(src: &str, profile: &str) {
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(state_file_src(src), format!("{}\n", profile));
    apply();
}

fn set_automation(state: &str) {
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(automation_file(), format!("{}\n", state));
    if state == "off" { restore_brightness(); }
    apply();
}

fn charge(args: &[String]) -> String {
    let cl = charge_limit_bin();
    let sudo_bin = sudo();
    let sub = args.get(0).map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "" | "show" => {
            // print ceiling
            // delegate to charge-limit binary
            let (code, out, err) = run(&cl, &[]);
            if code == 0 { out.trim().to_string() } else { format!("ERR {}", err.trim()) }
        }
        "full" => {
            let (code, _, err) = run(&sudo_bin, &[&cl, "full"]);
            if code != 0 { format!("ERR {}", err.trim()) } else { "OK".to_string() }
        }
        "limit" => {
            let n = args.get(1).cloned().unwrap_or_default();
            let (code, _, err) = if n.is_empty() {
                run(&sudo_bin, &[&cl, "limit"])
            } else {
                run(&sudo_bin, &[&cl, "limit", &n])
            };
            if code != 0 { format!("ERR {}", err.trim()) } else { "OK".to_string() }
        }
        "apply" => {
            let (code, _, err) = run(&cl, &["apply"]);
            if code != 0 { format!("ERR {}", err.trim()) } else { "OK".to_string() }
        }
        _ => format!("ERR charge wants show, full, limit [N] or apply"),
    }
}

fn show() -> String {
    let src = source_name();
    let (code, profile, _) = run(&ppd(), &["get"]);
    let profile = if code == 0 { profile.trim().to_string() } else { "unknown".to_string() };
    let mut out = String::new();
    out.push_str(&format!("source:  {}\n", src));
    out.push_str(&format!("profile: {}\n", profile));
    out.push_str(&format!("on mains:  {}\n", preferred("ac")));
    out.push_str(&format!("on battery: {}\n", preferred("battery")));
    if let Some(ceil) = charge_ceiling() {
        out.push_str(&format!("charge:    {}% ceiling\n", ceil));
    }
    if automation_enabled() {
        let bat = battery_percent().map(|v| v.to_string()).unwrap_or_else(|| "n/a".to_string());
        out.push_str(&format!("battery:   {}%\n", bat));
        out.push_str(&format!("automation: on (low {}% -> {} at {}%, critical {}% -> {} at {}%)\n",
            low_pct(), low_profile(), low_brightness(), critical_pct(), critical_profile(), critical_brightness()));
    } else {
        out.push_str("automation: off\n");
    }
    out.trim_end().to_string()
}

pub fn handle(args: &[String]) -> String {
    let sub = args.get(0).map(|s| s.as_str()).unwrap_or("");
    match sub {
        "" | "show" | "status" => show(),
        "apply" => { apply(); "OK".to_string() }
        "set" => {
            // power set ac|battery PROFILE
            if args.len() < 3 { return "ERR set needs a source and a profile".to_string(); }
            let src = &args[1];
            let profile = &args[2];
            if src != "ac" && src != "battery" { return format!("ERR unknown source \"{}\"", src); }
            set_pref(src, profile);
            "OK".to_string()
        }
        "automation" => if automation_enabled() { "on".to_string() } else { "off".to_string() },
        "set-automation" => {
            if args.len() < 2 { return "ERR set-automation needs on or off".to_string(); }
            match args[1].as_str() {
                "on" | "off" => { set_automation(&args[1]); "OK".to_string() }
                _ => format!("ERR unknown automation state \"{}\"", args[1]),
            }
        }
        "charge" => {
            let rest: Vec<String> = args[1..].to_vec();
            charge(&rest)
        }
        "check" => { check_battery(); "OK".to_string() }
        "watch" => {
            // delegate to bash powerprofile watch (udev loop) for now
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run("powerprofile", &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run("powerprofile", &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}
