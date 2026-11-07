//! Reminders via systemd user timers + notify-send.

use crate::proc::{find_in_path, notify_send, quickshell as qs, run};
use std::env;

fn systemctl() -> String { env::var("SYSTEMCTL").unwrap_or_else(|_| "systemctl".to_string()) }
fn systemd_run() -> String { env::var("SYSTEMD_RUN").unwrap_or_else(|_| "systemd-run".to_string()) }
fn reminded_prefix() -> &'static str { "reminder" }

fn usage() -> String {
    "reminder 30 [message] | reminder show | count | clear | -i | --ask MIN".to_string()
}

fn abs_bin(name: &str) -> String {
    let path = env::var("PATH").unwrap_or_default();
    find_in_path(name, &path).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string())
}

fn timer_list() -> String {
    let (code, out, _) = run(&systemctl(), &[
        "--user", "list-timers", "--all", "--no-legend", "--no-pager", "reminder-*.timer",
    ]);
    if code == 0 { out } else { String::new() }
}

fn timer_count() -> usize {
    timer_list().lines().filter(|l| !l.trim().is_empty()).count()
}

fn push_count() {
    let n = timer_count();
    let _ = run(&qs(), &["ipc", "call", "indicator", "reminders", &n.to_string()]);
}

fn schedule(minutes: &str, message: &str) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let unit = format!("{}-{}-{}", reminded_prefix(), secs, std::process::id());
    let notify_bin = abs_bin(&notify_send());
    let qs_bin = abs_bin(&qs());
    let glyph = "\u{f017}";
    let exec = format!(r#"["{}", "ipc", "call", "panel", "toggle", "notifications"]"#, qs_bin);
    let desc = format!("Reminder: {}", message);
    let (code, _out, err) = run(&systemd_run(), &[
        "--user", "--quiet",
        &format!("--unit={}", unit),
        &format!("--description={}", desc),
        &format!("--on-active={}min", minutes),
        &notify_bin, "-a", "reminders", "-u", "normal",
        &format!("--hint=string:anixie-glyph:{}", glyph),
        "Reminder", message,
        &format!("--hint=string:anixie-exec:{}", exec),
    ]);
    if code != 0 {
        return format!("ERR {}", err.trim());
    }
    push_count();
    format!("Reminder in {} minute(s): {}", minutes, message)
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "-h" | "--help" | "help" => usage(),
        "-i" | "--interactive" => {
            let entries = "5\n10\n15\n30\n45\n60\n";
            crate::pick::handle_with_stdin(
                &[
                    "--title".to_string(),
                    "remind in minutes".to_string(),
                    "--on-select".to_string(),
                    "reminder --ask '{}'".to_string(),
                ],
                entries,
            )
        }
        "--ask" => {
            if args.len() < 2 {
                return "ERR usage: reminder --ask MIN".to_string();
            }
            if args[1].is_empty() || !args[1].chars().all(|c| c.is_ascii_digit()) {
                return "ERR usage: reminder --ask MIN".to_string();
            }
            crate::pick::handle_with_stdin(
                &[
                    "--title".to_string(),
                    "message".to_string(),
                    "--placeholder".to_string(),
                    "optional".to_string(),
                    "--input".to_string(),
                    "--allow-empty".to_string(),
                    "--on-select".to_string(),
                    format!("reminder {} '{{}}'", args[1]),
                ],
                "",
            )
        }
        "show" | "list" => {
            let out = timer_list();
            if out.trim().is_empty() {
                "No reminders set.".to_string()
            } else {
                out.trim_end().to_string()
            }
        }
        "count" => timer_count().to_string(),
        "clear" => {
            let (code, out, err) = run(&systemctl(), &[
                "--user", "list-units", "--all", "--plain", "--no-legend", "--no-pager", "reminder-*.timer",
            ]);
            if code == 0 {
                for line in out.lines() {
                    let unit = line.split_whitespace().next().unwrap_or("");
                    if !unit.is_empty() {
                        let _ = run(&systemctl(), &["--user", "stop", unit]);
                    }
                }
            } else if !err.trim().is_empty() {
                return format!("ERR {}", err.trim());
            }
            push_count();
            "OK".to_string()
        }
        "" => usage(),
        _ => {
            if !sub.chars().all(|c| c.is_ascii_digit()) {
                return format!("ERR {}", usage());
            }
            let message = if args.len() > 1 {
                args[1..].join(" ")
            } else {
                "Reminder".to_string()
            };
            schedule(sub, &message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_numeric_is_usage() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR"), "got: {out}");
    }

    #[test]
    fn help_returns_usage() {
        assert!(handle(&["--help".to_string()]).contains("reminder"));
    }

    #[test]
    fn ask_needs_minutes() {
        let out = handle(&["--ask".to_string()]);
        assert!(out.starts_with("ERR"), "got: {out}");
    }

    #[test]
    fn abs_bin_falls_back_to_name() {
        // Missing binary returns the name itself (run will fail later).
        assert_eq!(abs_bin("alexie-no-such-binary-xyz"), "alexie-no-such-binary-xyz");
    }
}
