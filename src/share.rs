use crate::proc::run;
use std::env;
use std::fs;
use std::path::PathBuf;

fn runtime_dir() -> PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        
        env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        })
    });
    PathBuf::from(rt).join("anixie")
}

fn last_capture() -> Result<String, String> {
    let p = runtime_dir().join("last-capture");
    let last = fs::read_to_string(&p).unwrap_or_default().trim().to_string();
    if last.is_empty() { return Err("No recent capture to share.".to_string()); }
    if !std::path::Path::new(&last).exists() { return Err("Recent capture not found.".to_string()); }
    Ok(last)
}

fn notify(msg: &str) {
    let _ = run("notify-send", &["-a", "capture", "-u", "low", "-t", "2000", "Share", msg]);
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "" | "menu" => {
            // Picker entries carry the subcommand as their value, so
            // the on-select runs `share <subcommand>` natively.
            let last = match last_capture() {
                Ok(l) => l,
                Err(e) => { let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", &e]); return format!("ERR {}", e); }
            };
            let _ = last;
            let entries = [
                "Copy file URI\turi",
                "Copy path\tpath",
                "Edit in satty\tedit",
                "Show in folder\tfolder",
                "Compress with transcode\tcompress",
            ];
            let data = entries.join("\n") + "\n";
            let out = crate::pick::handle_with_stdin(
                &["--title".to_string(), "Share capture".to_string(), "--on-select".to_string(), "share '{}'".to_string()],
                &data,
            );
            // Fallback without a picker: copy the path, like legacy.
            if out.starts_with("ERR") {
                let code = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/plain", &last, &[]);
                if code != 0 {
                    return format!("ERR (exit {}) wl-copy failed", code);
                }
                notify("Copied path to clipboard");
                return "OK".to_string();
            }
            out
        }
        "uri" => {
            let last = match last_capture() { Ok(l) => l, Err(e) => { let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", &e]); return format!("ERR {}", e); } };
            let uri = format!("file://{}", last);
            let code = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/uri-list", &format!("{}\n", uri), &[]);
            if code != 0 { return format!("ERR (exit {}) wl-copy failed", code); }
            notify("Copied file URI");
            uri
        }
        "path" => {
            let last = match last_capture() { Ok(l) => l, Err(e) => { let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", &e]); return format!("ERR {}", e); } };
            let code = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/plain", &last, &[]);
            if code != 0 { return format!("ERR (exit {}) wl-copy failed", code); }
            notify("Copied path");
            last
        }
        "edit" => {
            let last = match last_capture() { Ok(l) => l, Err(e) => { let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", &e]); return format!("ERR {}", e); } };
            let satty = env::var("SATTY").unwrap_or_else(|_| "satty".to_string());
            let _ = std::process::Command::new(&satty).args(["--filename", &last, "--output-filename", &last, "--initial-tool", "crop"]).spawn();
            "OK".to_string()
        }
        "folder" => {
            let last = match last_capture() { Ok(l) => l, Err(e) => { let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", &e]); return format!("ERR {}", e); } };
            let dir = std::path::Path::new(&last).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let _ = run("xdg-open", &[&dir]);
            "OK".to_string()
        }
        "compress" => {
            let last = match last_capture() { Ok(l) => l, Err(e) => { let _ = run("notify-send", &["-a", "capture", "-u", "normal", "-t", "3000", "Share", &e]); return format!("ERR {}", e); } };
            let _ = run("transcode", &[&last]);
            "OK".to_string()
        }
        _ => format!("ERR unknown share subcommand: {}", sub),
    }
}

fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }
