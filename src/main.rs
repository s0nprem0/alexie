mod dbus;
mod capture;
mod clipboard;
mod color;
mod config;
mod emoji;
mod focus;
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

/// Back-compat entry point for DBus `run(command: &str)` and any
/// external caller that still sends a whitespace-joined string.
/// Arguments containing spaces cannot survive this split — typed
/// DBus interfaces (or the JSON socket protocol below) should be
/// used for those. The socket protocol itself uses handle_args().
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

/// Socket protocol: one JSON object per line,
/// `{"args":[...],"stdin":"..."}`. Falls back to a whitespace-
/// joined plain string for back-compat (e.g. `echo ping | socat -`).
/// JSON preserves arguments containing spaces — the old
/// whitespace split broke paths, webapp names/URLs, and menu names.
#[derive(Debug, PartialEq)]
struct WireRequest {
    args: Vec<String>,
    stdin: String,
}

fn encode_request(args: &[String], stdin_data: &str) -> String {
    serde_json::json!({"args": args, "stdin": stdin_data}).to_string()
}

fn decode_request(line: &str) -> WireRequest {
    let trimmed = line.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(args) = value.get("args").and_then(|a| a.as_array()) {
            let argv: Vec<String> = args
                .iter()
                .filter_map(|a| a.as_str().map(|s| s.to_string()))
                .collect();
            let stdin = value
                .get("stdin")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            return WireRequest { args: argv, stdin };
        }
    }
    WireRequest {
        args: trimmed.split_whitespace().map(|s| s.to_string()).collect(),
        stdin: String::new(),
    }
}

fn run_daemon() {
    let sock = socket_path();
    if let Some(parent) = sock.parent() {
        let _ = fs::create_dir_all(parent);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    // Singleton check: if something already answers on the socket,
    // do not steal it by deleting the file and binding anyway.
    if sock.exists() {
        if UnixStream::connect(&sock).is_ok() {
            eprintln!("alexie daemon already running on {}", sock.display());
            std::process::exit(1);
        }
        let _ = fs::remove_file(&sock);
    }
    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("failed to bind socket: {}", e);
            std::process::exit(1);
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&sock, fs::Permissions::from_mode(0o600));
    }
    eprintln!("alexie daemon listening on {}", sock.display());

    // Start the optional DBus service in a background thread.
    std::thread::spawn(|| {
        dbus::run_dbus_server();
    });

    // One thread per connection: the old single-threaded loop
    // blocked every other client while a long command (a picker,
    // a screenshot, a wallpaper scan) was running.
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                std::thread::spawn(move || handle_stream(s));
            }
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
        if line.trim().is_empty() {
            continue;
        }
        let req = decode_request(&line);
        let resp = handle_args(req.args, &req.stdin);
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.write_all(b"\n__END__\n");
        let _ = stream.flush();
        break;
    }
}

fn run_client(args: &[String]) {
    use std::io::{IsTerminal, Read};
    // Forward piped stdin (e.g. `printf … | alexie pick …`); a tty
    // stdin is left alone so interactive commands are not blocked
    // waiting for EOF. Pick --input mode does not read stdin at all.
    let stdin_data = if std::io::stdin().is_terminal() {
        String::new()
    } else {
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        buf
    };
    let sock = socket_path();
    let mut stream = match UnixStream::connect(&sock) {
        Ok(s) => s,
        Err(_) => {
            // Daemon not running: execute locally so timers and scripts still work.
            let resp = handle_args(args.to_vec(), &stdin_data);
            let is_err = resp.starts_with("ERR");
            if is_err { println!("{}", color::red(&resp)); } else { println!("{}", resp); }
            std::process::exit(if is_err { 1 } else { 0 });
        }
    };
    let _ = stream.write_all(format!("{}\n", encode_request(args, &stdin_data)).as_bytes());
    let _ = stream.flush();
    let mut is_err = false;
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
            is_err = true;
            println!("{}", color::red(&line));
        } else {
            println!("{}", line);
        }
    }
    if is_err {
        std::process::exit(1);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_request_roundtrip_preserves_spaces() {
        // Regression: the old whitespace protocol split
        // `webapp add "My App" https://…` into separate args.
        let args = vec![
            "webapp".to_string(),
            "add".to_string(),
            "My App".to_string(),
            "https://example.com/a b".to_string(),
        ];
        let encoded = encode_request(&args, "stdin data\n");
        let decoded = decode_request(&encoded);
        assert_eq!(decoded.args, args);
        assert_eq!(decoded.stdin, "stdin data\n");
    }

    #[test]
    fn wire_request_backcompat_plain_string() {
        let decoded = decode_request("theme list");
        assert_eq!(decoded.args, vec!["theme".to_string(), "list".to_string()]);
        assert_eq!(decoded.stdin, "");
    }

    #[test]
    fn handle_args_preserves_spacey_args() {
        // `webapp add` with a spacey name must reach webapp::handle
        // as one argument, not two. It will fail URL validation
        // only if the split is wrong; here the URL is valid and the
        // name is accepted, so we get the installed/generic path
        // rather than a usage error from a split name. Use a name
        // with a space and check the error is about the URL, proving
        // the name arrived intact as args[1].
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
        // DBus run(command) still works for space-free commands.
        assert_eq!(handle_command("ping"), "pong");
        assert!(handle_command("theme list").contains("tokyo-night"));
    }
}
