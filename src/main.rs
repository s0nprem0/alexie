use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

fn socket_path() -> PathBuf {
    let runtime = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("alexie").join("daemon.sock")
}

fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}

fn display_conf_path() -> PathBuf {
    state_dir().join("display.conf")
}

fn handle_command(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.as_slice() {
        ["ping"] => "pong".to_string(),
        ["monitors", "status"] => {
            let path = display_conf_path();
            match fs::read_to_string(&path) {
                Ok(s) => if s.trim().is_empty() {
                    "no display.conf".to_string()
                } else {
                    s.trim_end().to_string()
                },
                Err(_) => "no display.conf".to_string(),
            }
        }
        ["monitors", "set", output, key, value] => {
            if !output.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
                return format!("ERR invalid output name: {}", output);
            }
            let field_idx: usize = match *key {
                "scale" => 1,
                "transform" => 2,
                "disabled" => 3,
                "mirror" => 4,
                "position" => 5,
                "mode" => 6,
                _ => return format!("ERR unknown monitor setting: {}", key),
            };
            if *key == "scale" {
                let scale: f64 = match value.parse() {
                    Ok(v) => v,
                    Err(_) => return format!("ERR invalid scale: {}", value),
                };
                if scale <= 0.0 {
                    return "ERR scale must be > 0".to_string();
                }
            }
            if *key == "disabled" && *value != "0" && *value != "1" {
                return "ERR disabled must be 0 or 1".to_string();
            }

            let path = display_conf_path();
            let mut lines: Vec<String> = fs::read_to_string(&path)
                .unwrap_or_default()
                .lines()
                .map(|s| s.to_string())
                .collect();

            let mut found = false;
            for line in lines.iter_mut() {
                let toks: Vec<&str> = line.split_whitespace().collect();
                if !toks.is_empty() && toks[0] == *output {
                    let mut fields: Vec<String> = toks.iter().map(|s| s.to_string()).collect();
                    while fields.len() < 9 {
                        let defaults = ["", "1", "0", "0", "none", "auto", "preferred", "", ""];
                        fields.push(defaults[fields.len()].to_string());
                    }
                    if field_idx < fields.len() {
                        fields[field_idx] = value.to_string();
                    }
                    *line = fields.join(" ");
                    found = true;
                }
            }
            if !found {
                let mut fields = vec![
                    output.to_string(),
                    "1".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "none".to_string(),
                    "auto".to_string(),
                    "preferred".to_string(),
                    "".to_string(),
                    "".to_string(),
                ];
                if field_idx < fields.len() {
                    fields[field_idx] = value.to_string();
                }
                lines.push(fields.join(" "));
            }
            let _ = fs::create_dir_all(state_dir());
            match fs::write(&path, lines.join("\n") + "\n") {
                Ok(_) => format!("OK set {} {} {}", output, key, value),
                Err(e) => format!("ERR write failed: {}", e),
            }
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