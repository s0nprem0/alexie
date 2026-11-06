use std::env;
use std::fs;
use std::process::Command;

fn qs() -> String { env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string()) }
fn jq() -> String { "jq".to_string() }

fn state_dir() -> std::path::PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    std::path::PathBuf::from(state).join("anixie")
}

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn format_seconds(total: i64) -> String {
    let hours = total / 3600;
    let mins = (total % 3600) / 60;
    if hours > 0 { format!("{}h {}m", hours, mins) } else { format!("{}m", mins) }
}

pub fn handle(args: &[String]) -> String {
    let sub = args.get(0).map(|s| s.as_str()).unwrap_or("show");
    let day = {
        let (code, out, _) = run("date", &["+%F"]);
        if code == 0 { out.trim().to_string() } else { String::new() }
    };
    let state = state_dir();
    let file = state.join(format!("{}.json", day));

    match sub {
        "" | "show" => {
            // try live IPC
            let (code, live, _) = run(&qs(), &["ipc", "call", "screentime", "status"]);
            let payload = if code == 0 && !live.trim().is_empty() {
                // validate json object via jq
                let (vc, _, _) = run("sh", &["-c", &format!("printf '%s' {} | jq -e 'type == \"object\"' >/dev/null 2>&1", shell_quote(&live))]);
                if vc == 0 { live.trim().to_string() } else if file.exists() { std::fs::read_to_string(&file).unwrap_or_default() } else { String::new() }
            } else if file.exists() { std::fs::read_to_string(&file).unwrap_or_default() } else { String::new() };

            if payload.is_empty() {
                return "No screen time recorded today.".to_string();
            }
            // parse active/idle
            let active: i64 = {
                let (c, o, _) = run("sh", &["-c", &format!("printf '%s' {} | jq -r '.active // 0'", shell_quote(&payload))]);
                if c == 0 { o.trim().parse().unwrap_or(0) } else { 0 }
            };
            let idle: i64 = {
                let (c, o, _) = run("sh", &["-c", &format!("printf '%s' {} | jq -r '.idle // 0'", shell_quote(&payload))]);
                if c == 0 { o.trim().parse().unwrap_or(0) } else { 0 }
            };
            let mut out = String::new();
            out.push_str(&format!("date:   {}\n", day));
            out.push_str(&format!("active: {}\n", format_seconds(active)));
            out.push_str(&format!("idle:   {}\n", format_seconds(idle)));
            out.push_str(&format!("total:  {}\n", format_seconds(active + idle)));
            out.push_str("\napps:\n");
            let apps_script = format!("printf '%s' {} | jq -r '.apps // {{}} | to_entries | sort_by(-.value) | .[:8][] | \"\\(.value)\\t\\(.key)\"'", shell_quote(&payload));
            let (c, o, _) = run("sh", &["-c", &apps_script]);
            if c == 0 {
                for line in o.lines() {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() >= 2 {
                        let secs: i64 = parts[0].parse().unwrap_or(0);
                        let app = parts[1];
                        if !app.is_empty() {
                            out.push_str(&format!("  {:<10} {}\n", format_seconds(secs), app));
                        }
                    }
                }
            }
            out.trim_end().to_string()
        }
        "json" => {
            let (code, live, _) = run(&qs(), &["ipc", "call", "screentime", "status"]);
            let payload = if code == 0 && !live.trim().is_empty() {
                let (vc, _, _) = run("sh", &["-c", &format!("printf '%s' {} | jq -e 'type == \"object\"' >/dev/null 2>&1", shell_quote(&live))]);
                if vc == 0 { live.trim().to_string() } else if file.exists() { std::fs::read_to_string(&file).unwrap_or_default() } else { String::new() }
            } else if file.exists() { std::fs::read_to_string(&file).unwrap_or_default() } else { String::new() };
            if payload.is_empty() {
                format!("{{\"date\":\"{}\",\"apps\":{{}},\"idle\":0,\"active\":0}}", day)
            } else { payload }
        }
        "reset" => {
            let _ = run(&qs(), &["ipc", "call", "screentime", "reset"]);
            let _ = std::fs::remove_file(&file);
            format!("Screen time reset for {}.", day)
        }
        "path" => file.to_string_lossy().to_string(),
        "panel" => {
            let _ = run(&qs(), &["ipc", "call", "bar", "enable", "bar.screentime"]);
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = run(&qs(), &["ipc", "call", "panel", "toggle", "screentime"]);
            "OK".to_string()
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run("screentime", &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}

fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
}
