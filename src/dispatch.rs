use super::help::{help_text, is_help_flag};
use crate::{apps, capture, clipboard, emoji, focus, menu, monitors, pick, power, proc, record, reminder, screentime, share, shortcuts, shot, snapshot, status, theme, toggle, transcode, wallpaper, webcam_overlay, webapp};
use std::process::Command;

fn run_tool(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(proc::resolve_tool(cmd)).args(args).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            (code, out, err)
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

pub fn handle_command(cmd: &str) -> String {
    let parts: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
    handle_args(parts, "")
}

/// Dispatch a parsed argument vector, with piped stdin data for
/// commands that read entries from it (currently `pick`).
pub fn handle_args(parts: Vec<String>, stdin_data: &str) -> String {
    match parts.first().map(|s| s.as_str()) {
        None | Some("--help") | Some("-h") | Some("help") => {
            help_text(parts.get(1).map(|s| s.as_str()))
        }
        Some(c) if parts.len() >= 2 && is_help_flag(&parts[1]) => {
            help_text(Some(c))
        }
        Some("ping") => "pong".to_string(),
        Some("monitors") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            monitors::handle(&args)
        }
        Some("theme") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            theme::handle(&args)
        }
        Some("wallpaper") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            wallpaper::handle(&args)
        }
        Some("focus") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            focus::handle(&args)
        }
        Some("snapshot") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            snapshot::handle(&args)
        }
        Some("webapp") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            webapp::handle(&args)
        }
        Some("power") | Some("powerprofile") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            power::handle(&args)
        }
        Some("screentime") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            screentime::handle(&args)
        }
        Some("status") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            status::handle(&args)
        }
        Some("network") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            status::network_handle(&args)
        }
        Some("sysmon") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            status::sysmon_handle(&args)
        }
        Some("audio") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            status::audio_handle(&args)
        }
        Some("bluetooth") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            status::bluetooth_handle(&args)
        }
        Some("shot") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            shot::handle(&args)
        }
        Some("apps") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            apps::handle(&args)
        }
        Some("record") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            record::handle(&args)
        }
        Some("transcode") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            transcode::handle(&args)
        }
        Some("webcam-overlay") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            webcam_overlay::handle(&args)
        }
        Some("clipboard") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            clipboard::handle(&args)
        }
        Some("emoji") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            emoji::handle(&args)
        }
        Some("pick") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            pick::handle_with_stdin(&args, stdin_data)
        }
        Some("reminder") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            reminder::handle(&args)
        }
        Some("share") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            share::handle(&args)
        }
        Some("shortcuts") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            shortcuts::handle(&args)
        }
        Some("capture") | Some("capture-text") | Some("capture-qr") => {
            let mut args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            // normalize capture-text / capture-qr into capture text/qr
            if parts[0] == "capture-text" { args.insert(0, "text".to_string()); }
            if parts[0] == "capture-qr" { args.insert(0, "qr".to_string()); }
            if args.is_empty() && parts[0] == "capture" { args.push("text".to_string()); }
            capture::handle(&args)
        }
        Some("toggle") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            toggle::handle(&args)
        }
        Some("menu") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            menu::handle(&args)
        }
        _ => {
            // Generic fallback: run the first token as a binary with the rest as
            // args, so unported tools work through the daemon without
            // individual ports. Known alexie commands are handled above
            // and never reach here, so this cannot re-enter alexie for
            // a shimmed name — except for a name that is *only* a shim
            // with no legacy, which the per-command handlers above now
            // reject as unknown instead of delegating here.
            let rest: Vec<&str> = parts[1..].iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run_tool(&parts[0], &rest);
            if code == 127 {
                format!("ERR unknown command: {}", parts.join(" "))
            } else if code == 0 {
                if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
            } else {
                format!("ERR (exit {}) {}", code, err.trim())
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_args_preserves_spacey_args() {
        let out = handle_args(
            vec![
                "webapp".to_string(),
                "add".to_string(),
                "My App".to_string(),
                "not-a-url".to_string(),
            ],
            "",
        );
        assert!(
            out.contains("http://"),
            "name with space must arrive intact, got: {out}"
        );
    }

    #[test]
    fn handle_command_backcompat_whitespace() {
        assert_eq!(handle_command("ping"), "pong");
        assert!(handle_command("theme list").contains("tokyo-night"));
    }
}
