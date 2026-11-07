//! Picker: send entries to quickshell via IPC.
//!
//! Native port of anixie `pick.sh`. Writes `entries` + flags as a
//! JSON request file and calls `quickshell ipc call pick open`.
//!
//! on-select contract: quickshell substitutes `{}` with the selected
//! value and runs the result through the shell. Templates must quote
//! the placeholder as `'{}'` (so values become a single shell word)
//! and run static args through `crate::proc::shell_quote` at build
//! time. Picker values must stay free of `'` — handlers validate
//! what they accept (e.g. emoji `apply`, reminder minutes).

use crate::proc::{run, quickshell as qs};
use std::path::PathBuf;

pub(crate) struct Request {
    pub title: String,
    pub placeholder: String,
    pub mode: String,
    pub grouped: bool,
    pub allow_empty: bool,
    pub on_select: String,
    pub on_cancel: String,
    pub entries_text: String,
}

pub(crate) fn parse_request(args: &[String]) -> Result<Request, String> {
    let mut title = String::new();
    let mut placeholder = String::new();
    let mut mode = "list".to_string();
    let mut grouped = false;
    let mut allow_empty = false;
    let mut on_select = String::new();
    let mut on_cancel = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => return Err("usage".to_string()),
            "--title" => {
                let v = args.get(i + 1).ok_or("ERR --title needs a value")?;
                title = v.clone();
                i += 2;
            }
            "--placeholder" => {
                let v = args.get(i + 1).ok_or("ERR --placeholder needs a value")?;
                placeholder = v.clone();
                i += 2;
            }
            "--input" => { mode = "input".to_string(); i += 1; }
            "--allow-empty" => { allow_empty = true; i += 1; }
            "--grouped" => { grouped = true; i += 1; }
            "--on-select" => {
                let v = args.get(i + 1).ok_or("ERR --on-select needs a command")?;
                on_select = v.clone();
                i += 2;
            }
            "--on-cancel" => {
                let v = args.get(i + 1).ok_or("ERR --on-cancel needs a command")?;
                on_cancel = v.clone();
                i += 2;
            }
            other => return Err(format!("ERR unknown argument: {} (try --help)", other)),
        }
    }
    Ok(Request {
        title,
        placeholder,
        mode,
        grouped,
        allow_empty,
        on_select,
        on_cancel,
        entries_text: String::new(),
    })
}

pub(crate) fn entries_to_json(stdin_data: &str) -> Vec<serde_json::Value> {
    stdin_data
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut parts: Vec<&str> = line.split('\t').collect();
            while parts.len() < 5 {
                parts.push("");
            }
            serde_json::json!({
                "label": parts[0],
                "value": if parts[1].is_empty() { parts[0] } else { parts[1] },
                "icon": parts[2],
                "state": parts[3],
                "preview": parts[4],
            })
        })
        .collect()
}

fn build_payload(req: &Request, stdin_data: &str) -> serde_json::Value {
    let entries = if req.mode == "input" {
        Vec::new()
    } else {
        entries_to_json(stdin_data)
    };
    serde_json::json!({
        "title": req.title,
        "placeholder": req.placeholder,
        "mode": req.mode,
        "grouped": req.grouped,
        "allowEmpty": req.allow_empty,
        "onSelect": req.on_select,
        "onCancel": req.on_cancel,
        "entries": entries,
    })
}

fn request_file(payload: &serde_json::Value) -> std::io::Result<PathBuf> {
    let path = crate::proc::tmp_file("pick-request", "json");
    std::fs::write(&path, serde_json::to_string(payload).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?)?;
    Ok(path)
}

pub fn handle_with_stdin(args: &[String], stdin_data: &str) -> String {
    let mut req = match parse_request(args) {
        Ok(r) => r,
        Err(e) if e == "usage" => return "usage: pick --title TITLE --on-select CMD [entries on stdin]".to_string(),
        Err(e) => return e,
    };
    if req.mode != "input" {
        let count = stdin_data.lines().filter(|l| !l.is_empty()).count();
        if count == 0 {
            return "ERR nothing to pick from".to_string();
        }
    }
    req.entries_text = stdin_data.to_string();
    let payload = build_payload(&req, stdin_data);
    let req_path = match request_file(&payload) {
        Ok(p) => p,
        Err(e) => return format!("ERR {}", e),
    };
    let (code, out, _err) = run(&qs(), &["ipc", "call", "pick", "open", &req_path.to_string_lossy()]);
    if code != 0 {
        let _ = std::fs::remove_file(&req_path);
        return "ERR the shell is not running".to_string();
    }
    let reply = out.trim();
    if reply == "ok" || reply.starts_with("ok") {
        // Shell removes the request file after reading.
        "OK".to_string()
    } else {
        let _ = std::fs::remove_file(&req_path);
        if reply.is_empty() {
            "ERR the shell did not take the request".to_string()
        } else {
            format!("ERR the shell did not take the request (said: {})", reply)
        }
    }
}

// CLI fallback symmetry with other modules; dispatch uses handle_with_stdin.
#[allow(dead_code)]
pub fn handle(args: &[String]) -> String {
    handle_with_stdin(args, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_basic() {
        let r = parse_request(&[
            "--title".into(), "t".into(),
            "--on-select".into(), "theme '{}'".into(),
        ]).unwrap();
        assert_eq!(r.title, "t");
        assert_eq!(r.mode, "list");
    }

    #[test]
    fn parse_request_input_mode() {
        let r = parse_request(&["--input".into(), "--allow-empty".into()]).unwrap();
        assert_eq!(r.mode, "input");
        assert!(r.allow_empty);
    }

    #[test]
    fn unknown_flag_fails_fast() {
        let out = handle_with_stdin(&["--__bogus__".into()], "");
        assert!(out.starts_with("ERR unknown argument:"), "got: {out}");
    }

    #[test]
    fn empty_entries_fails_fast() {
        let out = handle_with_stdin(&["--title".into(), "t".into(), "--on-select".into(), "x".into()], "");
        assert!(out.starts_with("ERR nothing to pick from"), "got: {out}");
    }

    #[test]
    fn entries_json_parses_tabs() {
        let v = entries_to_json("label\nlabel\tvalue\nlabel\tvalue\ticon");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0]["value"], "label");
        assert_eq!(v[1]["value"], "value");
        assert_eq!(v[2]["icon"], "icon");
    }

    #[test]
    fn payload_input_has_no_entries() {
        let mut r = parse_request(&["--input".into()]).unwrap();
        r.entries_text = "ignored\n".into();
        let p = build_payload(&r, "ignored\n");
        assert_eq!(p["entries"].as_array().unwrap().len(), 0);
    }
}
