use super::color;
use super::daemon::{encode_request, socket_path};
use super::dispatch::handle_args;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

pub(crate) fn run_client(args: &[String]) {
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

