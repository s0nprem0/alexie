use super::dispatch::handle_args;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub(crate) fn socket_path() -> PathBuf {
    let runtime = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime).join("alexie").join("daemon.sock")
}

/// Socket protocol: one JSON object per line,
/// `{"args":[...],"stdin":"..."}`. Falls back to a whitespace-
/// joined plain string for back-compat (e.g. `echo ping | socat -`).
/// JSON preserves arguments containing spaces — the old
/// whitespace split broke paths, webapp names/URLs, and menu names.
#[derive(Debug, PartialEq)]
pub(crate) struct WireRequest {
    args: Vec<String>,
    stdin: String,
}

pub(crate) fn encode_request(args: &[String], stdin_data: &str) -> String {
    serde_json::json!({"args": args, "stdin": stdin_data}).to_string()
}

pub(crate) fn decode_request(line: &str) -> WireRequest {
    let trimmed = line.trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(args) = value.get("args").and_then(|a| a.as_array())
    {
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
    WireRequest {
        args: trimmed.split_whitespace().map(|s| s.to_string()).collect(),
        stdin: String::new(),
    }
}

pub(crate) fn run_daemon() {
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
        crate::dbus::run_dbus_server();
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
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let reader = BufReader::new(clone);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_request_roundtrip_preserves_spaces() {
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
    fn socket_roundtrip_preserves_spaces_and_stdin() {
        use std::io::{BufRead, BufReader, Write as IoWrite};
        let dir = std::env::temp_dir().join(format!(
            "alexie-sock-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let sock_path = dir.join("daemon.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_stream(stream);
        });
        let mut client = UnixStream::connect(&sock_path).unwrap();
        let args = vec![
            "webapp".to_string(),
            "add".to_string(),
            "My App".to_string(),
            "not-a-url".to_string(),
        ];
        client
            .write_all(format!("{}\n", encode_request(&args, "")).as_bytes())
            .unwrap();
        client.flush().unwrap();
        let reader = BufReader::new(client);
        let mut lines = Vec::new();
        for line in reader.lines() {
            let line = line.unwrap();
            if line == "__END__" {
                break;
            }
            lines.push(line);
        }
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(lines.join("\n").contains("http://"), "got: {:?}", lines);
    }
}
