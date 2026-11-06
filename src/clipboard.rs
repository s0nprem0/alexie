use std::env;
use std::fs;
use std::process::Command;

fn resolve_tool(name: &str) -> String {
    // When the daemon is enabled, bash tools are renamed to *-legacy. Prefer the
    // legacy binary so delegation never recurses through the alexie shim.
    let legacy = format!("{}-legacy", name);
    let check2 = format!("command -v {} >/dev/null 2>&1", legacy);
    if Command::new("sh").args(["-c", &check2]).status().map(|s| s.success()).unwrap_or(false) {
        return legacy;
    }
    let check = format!("command -v {} >/dev/null 2>&1", name);
    if Command::new("sh").args(["-c", &check]).status().map(|s| s.success()).unwrap_or(false) {
        return name.to_string();
    }
    name.to_string()
}

fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' { out.push_str("'\\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
}

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(resolve_tool(cmd)).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn cliphist() -> String { env::var("CLIPHIST").unwrap_or_else(|_| "cliphist".to_string()) }
fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }
fn file_bin() -> String { env::var("FILE").unwrap_or_else(|_| "file".to_string()) }
fn qs() -> String { env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string()) }

fn runtime_dir() -> std::path::PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(rt)
}

fn preview_dir() -> std::path::PathBuf {
    runtime_dir().join(format!("anixie-clipboard-{}", uid()))
}

fn uid() -> String {
    let (code, out, _) = run("id", &["-u"]);
    if code == 0 { out.trim().to_string() } else { "1000".to_string() }
}

pub fn handle(args: &[String]) -> String {
    let sub = args.get(0).map(|s| s.as_str()).unwrap_or("");
    match sub {
        "--copy" => {
            if args.len() < 2 { return "ERR usage: clipboard --copy ID".to_string(); }
            let id = &args[1];
            // decode to temp
            let tmp = format!("/tmp/anixie-clip-{}", std::process::id());
            let (code, _, err) = run(&cliphist(), &["decode", id]);
            // cliphist decode writes to stdout; capture via sh
            let (code, out, err) = run("sh", &["-c", &format!("{} decode {} > {}", cliphist(), id, tmp)]);
            if code != 0 { let _ = std::fs::remove_file(&tmp); return format!("ERR {}", err.trim()); }
            if std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) == 0 {
                let _ = std::fs::remove_file(&tmp);
                return format!("ERR entry {} is empty", id);
            }
            // mime
            let (code, mime, _) = run(&file_bin(), &["-b", "--mime-type", "--", &tmp]);
            let mime = if code == 0 { mime.trim().to_string() } else { "application/octet-stream".to_string() };
            // copy
            let _ = run("sh", &["-c", &format!("{} --type {} < {}", wl_copy(), mime, tmp)]);
            let _ = std::fs::remove_file(&tmp);
            "OK".to_string()
        }
        "--list" => {
            let (code, out, err) = run(&cliphist(), &["list"]);
            if code != 0 { format!("ERR {}", err.trim()) } else { out.trim_end().to_string() }
        }
        "--text" => {
            if args.len() < 2 { return "ERR usage: clipboard --text ID".to_string(); }
            let (code, out, err) = run(&cliphist(), &["decode", &args[1]]);
            if code != 0 { format!("ERR {}", err.trim()) } else { out }
        }
        "--images" => {
            let dir = preview_dir().join("thumbs");
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            // chmod 700
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
            }
            let (code, list, _) = run(&cliphist(), &["list"]);
            if code != 0 { return "ERR cliphist list failed".to_string(); }
            let mut out = String::new();
            let mut count = 0;
            for line in list.lines() {
                if count >= 16 { break; }
                let tab = match line.find('\t') { Some(t) => t, None => continue };
                let id = &line[..tab];
                let preview = &line[tab+1..];
                if !preview.starts_with("[[ binary data ") { continue; }
                let path = dir.join(id);
                // decode
                let (dcode, _, _) = run("sh", &["-c", &format!("{} decode {} > {} 2>/dev/null", cliphist(), id, path.display())]);
                if dcode == 0 {
                    // check non-empty
                    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 0 {
                        out.push_str(&format!("{}\t{}\n", id, path.display()));
                        count += 1;
                    } else {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            out.trim_end().to_string()
        }
        "--image" => {
            if args.len() < 2 { return "ERR usage: clipboard --image ID".to_string(); }
            let dir = preview_dir().join("previews");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("preview-{}", args[1]));
            let (code, _, err) = run("sh", &["-c", &format!("{} decode {} > {} 2>/dev/null", cliphist(), args[1], path.display())]);
            if code != 0 { let _ = std::fs::remove_file(&path); return format!("ERR {}", err.trim()); }
            if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) == 0 {
                let _ = std::fs::remove_file(&path);
                return "ERR empty".to_string();
            }
            path.to_string_lossy().to_string()
        }
        "--delete" => {
            if args.len() < 2 { return "ERR usage: clipboard --delete ID".to_string(); }
            let id = &args[1];
            let (code, list, _) = run(&cliphist(), &["list"]);
            if code != 0 { return "ERR cliphist list failed".to_string(); }
            // find the line
            let mut found_line = String::new();
            for line in list.lines() {
                let tab = match line.find('\t') { Some(t) => t, None => continue };
                if &line[..tab] == id {
                    found_line = line.to_string();
                    break;
                }
            }
            if found_line.is_empty() { return "OK".to_string(); }
            let (code, _, err) = run("sh", &["-c", &format!("printf '%s\\n' {} | {} delete", shell_quote(&found_line), cliphist())]);
            if code != 0 { format!("ERR {}", err.trim()) } else { "OK".to_string() }
        }
        "--wipe" => {
            let (code, _, err) = run(&cliphist(), &["wipe"]);
            if code != 0 { format!("ERR {}", err.trim()) } else { "OK".to_string() }
        }
        _ => {
            // open popup via quickshell (default behavior)
            let (code, out, err) = run(&qs(), &["ipc", "call", "clipboard", "open"]);
            if code != 0 { format!("ERR (exit {}) {}", code, err.trim()) }
            else {
                let r = out.trim();
                match r {
                    "ok" | "\"ok\"" => "OK".to_string(),
                    _ => if r.is_empty() { "OK".to_string() } else { r.to_string() }
                }
            }
        }
    }
}
