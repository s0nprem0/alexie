mod dbus;
mod capture;
mod focus;
mod menu;
mod monitors;
mod screentime;
mod shot;
mod theme;
mod toggle;
mod wallpaper;

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use std::process::Command;

fn run_tool(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let err = String::from_utf8_lossy(&o.stderr).to_string();
            (code, out, err)
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
}


pub fn resolve_tool(name: &str) -> String {
    // Prefer a "*-legacy" binary when present (installed alongside the alexie
    // shim) so delegation does not recurse through the shim.
    let legacy = format!("{}-legacy", name);
    let (code, _, _) = run_tool("sh", &["-c", &format!("command -v {} >/dev/null 2>&1", legacy)]);
    if code == 0 { legacy } else { name.to_string() }
}

fn socket_path() -> PathBuf {
    let runtime = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("alexie").join("daemon.sock")
}

pub fn handle_command(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.first().copied() {
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
        Some("screentime") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            screentime::handle(&args)
        }
        Some("shot") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            shot::handle(&args)
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
        Some(tool) if ["shot", "record", "transcode", "capture-text", "capture-qr", "share",
                        "webcam-overlay", "clipboard", "emoji", "focus", "screentime",
                        "toggle", "power", "snapshot", "webapp"].contains(&tool) => {
            // Delegate to the installed bash tool until ported.
            let (code, out, err) = run_tool(&resolve_tool(parts[0]), &parts[1..]);
            if code == 0 {
                if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
            } else {
                format!("ERR (exit {}) {}", code, err.trim())
            }
        }
        _ => {
            // Generic fallback: run the first token as a binary with the rest as
            // args, so unported tools (clipboard, emoji, toggle, shot, ...) work
            // through the daemon without individual ports.
            let (code, out, err) = run_tool(parts[0], &parts[1..]);
            if code == 127 {
                format!("ERR unknown command: {}", cmd)
            } else if code == 0 {
                if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
            } else {
                format!("ERR (exit {}) {}", code, err.trim())
            }
        }
    }
}

fn run_daemon() {
    let sock = socket_path();
    if let Some(parent) = sock.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(&sock);
    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("failed to bind socket: {}", e);
            std::process::exit(1);
        }
    };
    eprintln!("alexie daemon listening on {}", sock.display());

    // Start the optional DBus service in a background thread.
    std::thread::spawn(|| {
        dbus::run_dbus_server();
    });

    for stream in listener.incoming() {
        match stream {
            Ok(s) => handle_stream(s),
            Err(e) => eprintln!("accept error: {}", e),
        }
    }
}

fn handle_stream(mut stream: UnixStream) {
    let reader = BufReader::new(stream.try_clone().unwrap());
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let cmd = line.trim().to_string();
        if cmd.is_empty() {
            continue;
        }
        let resp = handle_command(&cmd);
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.write_all(b"\n__END__\n");
        let _ = stream.flush();
        break;
    }
}

fn run_client(args: &[String]) {
    let cmd = args.join(" ");
    let sock = socket_path();
    let mut stream = match UnixStream::connect(&sock) {
        Ok(s) => s,
        Err(_) => {
            // Daemon not running: execute locally so timers and scripts still work.
            let cmd = args.join(" ");
            let resp = handle_command(&cmd);
            println!("{}", resp);
            std::process::exit(0);
        }
    };
    let _ = stream.write_all(format!("{}\n", cmd).as_bytes());
    let _ = stream.flush();
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line == "__END__" {
            break;
        }
        println!("{}", line);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: alexie <command...> | alexie daemon");
        std::process::exit(2);
    }
    if args[1] == "daemon" {
        run_daemon();
    } else {
        run_client(&args[1..]);
    }
}