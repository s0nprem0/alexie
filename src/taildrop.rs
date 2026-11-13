//! Taildrop receiver: long-running loop that pulls files sent via
//! Tailscale Taildrop into `$XDG_DOWNLOAD_DIR` (or `~/Downloads`).
//!
//! Native port of anixie's `taildrop-receive.sh`. Runs until killed;
//! intended to be launched by the `anixie-taildrop` user service.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

fn tool(name: &str, default: &str) -> String {
    crate::proc::tool(name, default)
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn downloads_dir() -> PathBuf {
    if let Ok(d) = env::var("XDG_DOWNLOAD_DIR") {
        PathBuf::from(d)
    } else {
        home_dir().join("Downloads")
    }
}

fn backend_state() -> String {
    let bin = tool("TAILSCALE", "tailscale");
    let out = Command::new(&bin).args(["status", "--json"]).output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("BackendState").and_then(|x| x.as_str()).map(str::to_string))
                .unwrap_or_else(|| "Unknown".to_string())
        }
        Err(_) => "Unknown".to_string(),
    }
}

fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn claim(source: &Path, downloads: &Path) -> Option<PathBuf> {
    let name = source.file_name()?.to_string_lossy().to_string();
    let (base, ext) = split_name(&name);
    for index in 0..1000usize {
        let candidate = if index == 0 {
            downloads.join(&name)
        } else {
            downloads.join(format!("{}-{}{}", base, index, ext))
        };
        if fs::hard_link(source, &candidate).is_ok() {
            let _ = fs::remove_file(source);
            return Some(candidate);
        }
        if !candidate.exists() {
            return None;
        }
    }
    None
}

fn is_image(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.rsplit('.').next(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "tif" | "tiff")
    )
}

fn announce(path: &Path, downloads: &Path) {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let xdg_open = tool("XDG_OPEN", "xdg-open");
    let notify = tool("NOTIFY_SEND", "notify-send");
    let path_str = path.to_string_lossy().to_string();
    let mut args: Vec<String> = Vec::new();
    if is_image(&name) {
        args.push("-i".to_string());
        args.push(path_str.clone());
    }
    args.push("-g".to_string());
    args.push("󰒊".to_string());
    args.push("-u".to_string());
    args.push("critical".to_string());
    args.push(format!("Taildrop: {}", name));
    args.push(format!("Saved to {}", downloads.display()));
    args.push("--exec".to_string());
    args.push(xdg_open);
    args.push(path_str);
    let _ = Command::new(&notify).args(&args).status();
}

fn deliver(downloads: &Path, staging: &Path) {
    let Ok(rd) = fs::read_dir(staging) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(target) = claim(&path, downloads) {
            announce(&target, downloads);
        }
    }
}

pub fn handle(args: &[String]) -> String {
    if args.iter().any(|a| matches!(a.as_str(), "-h" | "--help" | "help")) {
        return "usage: taildrop\n\n  Receive Taildrop files into the Downloads directory (runs until killed).".to_string();
    }

    let downloads = downloads_dir();
    let staging = downloads.join(".taildrop");
    let _ = fs::create_dir_all(&downloads);
    let _ = fs::create_dir_all(&staging);

    deliver(&downloads, &staging);

    loop {
        if backend_state() != "Running" {
            thread::sleep(Duration::from_secs(60));
            continue;
        }
        let tailscale = tool("TAILSCALE", "tailscale");
        let staging_str = staging.to_string_lossy().to_string();
        let status = Command::new(&tailscale)
            .args(["file", "get", "--wait", "--conflict=rename", &staging_str])
            .status();
        if status.map(|s| s.success()).unwrap_or(false) {
            deliver(&downloads, &staging);
        } else {
            thread::sleep(Duration::from_secs(60));
        }
    }
    #[allow(unreachable_code)]
    "OK".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_name_basic() {
        assert_eq!(split_name("foo.png"), ("foo", ".png"));
        assert_eq!(split_name("archive.tar.gz"), ("archive.tar", ".gz"));
        assert_eq!(split_name("README"), ("README", ""));
    }

    #[test]
    fn is_image_extensions() {
        assert!(is_image("a.PNG"));
        assert!(is_image("b.jpg"));
        assert!(!is_image("c.txt"));
    }

    #[test]
    fn handle_help() {
        let out = handle(&["--help".to_string()]);
        assert!(out.contains("usage: taildrop"));
    }
}
