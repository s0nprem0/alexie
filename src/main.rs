mod dbus;
mod capture;
mod clipboard;
mod color;
mod config;
mod emoji;
mod focus;
mod fmt;
mod format;
mod pick;
mod reminder;
mod share;
mod shortcuts;
mod snapshot;
mod table;
mod webapp;
mod menu;
mod webcam_overlay;
mod transcode;
mod record;
mod proc;
mod monitors;
mod power;
mod power_watch;
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


fn socket_path() -> PathBuf {
    let runtime = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("alexie").join("daemon.sock")
}

fn is_help_flag(s: &str) -> bool {
    matches!(s, "--help" | "-h" | "help")
}

fn help_text(cmd: Option<&str>) -> String {
    match cmd {
        None => {
            let mut s = String::new();
            s.push_str("usage: alexie <command...> | alexie daemon\n\n");
            s.push_str("commands:\n");
            for (name, desc, _) in COMMANDS {
                s.push_str(&format!("  {:<15} {}\n", name, desc));
            }
            s.push_str("\nhelp: alexie --help | alexie <command> --help | alexie help <command>\n");
            s
        }
        Some(c) => {
            if let Some((_, desc, subs)) = COMMANDS.iter().find(|(n, _, _)| *n == c) {
                let mut s = String::new();
                s.push_str(&format!("usage: alexie {} <subcommand>...\n\n", c));
                s.push_str(&format!("{}\n\n", desc));
                s.push_str("subcommands:\n");
                s.push_str(subs);
                if !subs.ends_with('\n') { s.push('\n'); }
                s
            } else {
                format!("unknown command: {}\n\n{}", c, help_text(None))
            }
        }
    }
}

const COMMANDS: &[(&str, &str, &str)] = &[
    ("monitors", "display scaling, mode, position, mirroring, workspaces.", "  status, list, set OUTPUT KEY VALUE\n  scale set|up|down|reset\n  brightness up|down|max|min|set N\n  identify, wake, recover\n  internal on|off|toggle\n  mirror on|off|toggle\n  lid close|open|toggle|reconcile\n  try OUTPUT KEY VALUE, keep, revert\n  workspaces sync|show|reset\n  workspace N, workspace-step next|prev\n  move-window N, identities, sync-identities\n"),
    ("theme", "palette listing and switching.", "  list, current, next, prev, state, list --json\n  switch THEME\n  export, doctor, vars, render\n"),
    ("wallpaper", "wallpaper set/watch and library.", "  current, status, set FILE, list, library\n  next, prev, rotate, pause, resume\n  watch, watch-foreground\n"),
    ("power", "power profiles and charge limits.", "  show|status, apply, set ac|battery PROFILE\n  charge full|limit N\n  watch\n"),
    ("screentime", "screen time tracking.", "  show, json, reset, path, panel, watch\n"),
    ("focus", "focus mode.", "  start, stop, extend, status\n"),
    ("snapshot", "btrfs snapshots.", "  create, list, restore, prune\n"),
    ("webapp", "websites as apps.", "  add, list, remove\n"),
    ("shot", "screenshots.", "  smart, region, windows, fullscreen, window\n  copy, save, preview, annotate, redact, text\n"),
    ("record", "screen recording.", "  --status, --stop, --fullscreen, --audio\n"),
    ("transcode", "picture/video resize.", "  picture|video ...\n"),
    ("webcam-overlay", "webcam overlay.", "  start, stop, resize, list\n"),
    ("clipboard", "clipboard history.", "  --copy, --list, --text, --image\n"),
    ("emoji", "emoji picker.", "  (no subcommands)\n"),
    ("pick", "generic picker.", "  --title, --placeholder, --input\n"),
    ("reminder", "reminders.", "  show, count, clear\n"),
    ("share", "file sharing.", "  uri, path, edit, folder, compress\n"),
    ("shortcuts", "keyboard shortcuts.", "  (help)\n"),
    ("toggle", "nightlight, idle, dnd, stay-awake.", "  nightlight, idle, dnd, stayawake\n  status, list\n"),
    ("menu", "application and action menu.", "  apps, --list, --only\n"),
    ("capture", "capture text/qr/share.", "  text, qr, share\n"),
    ("daemon", "run the alexie daemon.", "  (no subcommands)\n"),
    ("ping", "health check.", "  (no subcommands)\n"),
];

pub fn handle_command(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.first().copied() {
        None | Some("--help") | Some("-h") | Some("help") => {
            return help_text(parts.get(1).copied());
        }
        Some(c) if parts.len() >= 2 && is_help_flag(parts[1]) => {
            return help_text(Some(c));
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
        Some("shot") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            shot::handle(&args)
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
            pick::handle(&args)
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
            if resp.starts_with("ERR") { println!("{}", color::red(&resp)); } else { println!("{}", resp); }
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
        if line.starts_with("ERR") {
            println!("{}", color::red(&line));
        } else {
            println!("{}", line);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 && args[1] == "help" {
        print!("{}", help_text(Some(&args[2])));
        std::process::exit(0);
    }
    if args.len() < 2 || is_help_flag(&args[1]) || args[1] == "--help" || args[1] == "-h" {
        print!("{}", help_text(None));
        std::process::exit(0);
    }
    if args.len() >= 3 && is_help_flag(&args[2]) {
        print!("{}", help_text(Some(&args[1])));
        std::process::exit(0);
    }

    // power watch: run the watcher (blocks).
    if args.len() >= 3 && (args[1] == "power" || args[1] == "powerprofile") && args[2] == "watch" {
        power_watch::run();
        return;
    }
    // screentime watch: run the tracker (blocks).
    if args.len() >= 3 && args[1] == "screentime" && args[2] == "watch" {
        screentime::watch();
        return;
    }
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
