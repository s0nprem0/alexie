mod monitors;

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

fn socket_path() -> PathBuf {
    let runtime = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("alexie").join("daemon.sock")
}

fn handle_command(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.first().copied() {
        Some("ping") => "pong".to_string(),
        Some("monitors") => {
            let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            monitors::handle(&args)
        }
        _ => format!("ERR unknown command: {}", cmd),
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
            eprintln!("daemon not running ({}). Start with: alexie daemon", sock.display());
            std::process::exit(1);
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