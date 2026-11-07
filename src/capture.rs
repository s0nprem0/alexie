use crate::proc::run;
use std::env;


fn shot_pick() -> String {
    env::var("SHOT_PICK").unwrap_or_else(|_| "shot-pick".to_string())
}
fn grim() -> String { env::var("GRIM").unwrap_or_else(|_| "grim".to_string()) }
fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }
fn notify() -> String { env::var("NOTIFY_SEND").unwrap_or_else(|_| "notify-send".to_string()) }


fn runtime_dir() -> std::path::PathBuf {
    let rt = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        
        env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        })
    });
    std::path::PathBuf::from(rt).join("anixie")
}

pub fn handle(args: &[String]) -> String {
    // Bare `alexie capture` defaults to text capture (the hotkey path).
    // main.rs already inserts "text", but keep this consistent for any
    // other caller instead of erroring with "missing subcommand".
    let default_args;
    let args = if args.is_empty() {
        default_args = vec!["text".to_string()];
        default_args.as_slice()
    } else {
        args
    };
    match args[0].as_str() {
        "text" => capture_text(),
        "qr" => capture_qr(),
        "share" => {
            // native share (subcommands); delegate unknown to legacy via share::handle
            let sub_args: Vec<String> = args[1..].to_vec();
            crate::share::handle(&sub_args)
        }
        "cancel" | "edit-last" | "copy-path" | "copy-uri" => {
            // These modes are already native in shot::handle. Calling
            // shot::handle directly avoids spawning the `shot` binary,
            // which via resolve_tool("shot") could re-enter alexie
            // through the shot shim, and avoids depending on shot-legacy.
            crate::shot::handle(args)
        }
        _ => format!("ERR unknown subcommand: {}", args[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }
}

fn capture_text() -> String {
    let sp = shot_pick();
    let freeze_file = runtime_dir().join("anixie-pick-freeze.pid");
    // get geometry
    let (code, geom, err) = run(&sp, &["region", "--keep-freeze"]);
    if code != 0 { release_freeze(&freeze_file); return format!("ERR pick: {}", err.trim()); }
    let geom = geom.trim().to_string();
    if geom.is_empty() { release_freeze(&freeze_file); return "ERR no region".to_string(); }

    // grim -g geom - | tesseract
    let grim_bin = grim();
    let tesseract_bin = env::var("TESSERACT").unwrap_or_else(|_| "tesseract".to_string());
    let dpi = env::var("CAPTURE_TEXT_DPI").unwrap_or_else(|_| "300".to_string());
    let langs = env::var("CAPTURE_TEXT_LANGS").unwrap_or_else(|_| "eng".to_string());

    // Run grim to temp file, then tesseract
    let tmp = crate::proc::tmp_file("text", "png").to_string_lossy().to_string();
    let (code, _, err) = run(&grim_bin, &["-g", &geom, &tmp]);
    if code != 0 {
        // release freeze
        release_freeze(&freeze_file);
        return format!("ERR grim: {}", err.trim());
    }

    let (_code, text, _) = run(&tesseract_bin, &[
        &tmp, "stdout", "--oem", "1", "--psm", "6", "--dpi", &dpi,
        "-l", &langs, "-c", "preserve_interword_spaces=1",
    ]);
    let _ = std::fs::remove_file(&tmp);

    // release freeze
    release_freeze(&freeze_file);

    let text = text.trim().to_string();
    if text.is_empty() {
        let _ = run(&notify(), &["-u", "normal", "-a", "capture", "-G", "f071", "No text found", "Nothing in that region was readable."]);
        return "ERR no text found".to_string();
    }

    // copy to clipboard
    let _ = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/plain", &text, &[]);
    let first = text.lines().next().unwrap_or("").chars().take(60).collect::<String>();
    let _ = run(&notify(), &["-a", "capture", "-G", "f0c5", "Text copied", &first]);
    "OK".to_string()
}

fn capture_qr() -> String {
    let sp = shot_pick();
    let freeze_file = runtime_dir().join("anixie-pick-freeze.pid");
    let (code, geom, err) = run(&sp, &["region", "--keep-freeze"]);
    if code != 0 { release_freeze(&freeze_file); return format!("ERR pick: {}", err.trim()); }
    let geom = geom.trim().to_string();
    if geom.is_empty() { release_freeze(&freeze_file); return "ERR no region".to_string(); }

    let grim_bin = grim();
    let zbar_bin = env::var("ZBAR").unwrap_or_else(|_| "zbarimg".to_string());
    let tmp = crate::proc::tmp_file("qr", "png").to_string_lossy().to_string();
    let (code, _, err) = run(&grim_bin, &["-g", &geom, &tmp]);
    if code != 0 { release_freeze(&freeze_file); let _ = std::fs::remove_file(&tmp); return format!("ERR grim: {}", err.trim()); }

    let (_code, result, _) = {
        // zbarimg reads the image on stdin; feed the temp file
        // directly instead of `zbarimg … - < tmp` via a shell.
        use std::process::Stdio;
        let res = std::fs::File::open(&tmp).ok().and_then(|f| {
            std::process::Command::new(&zbar_bin)
                .args(["-q", "--raw", "-Sdisable", "-Sqrcode.enable", "-"])
                .stdin(Stdio::from(f))
                .output()
                .ok()
        });
        match res {
            Some(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
            None => (-1, String::new(), String::new()),
        }
    };
    let _ = std::fs::remove_file(&tmp);
    release_freeze(&freeze_file);

    let result = result.trim().to_string();
    if result.is_empty() {
        let _ = run(&notify(), &["-u", "normal", "-a", "capture", "-G", "f071", "No QR code found", "Nothing in that region decoded as a QR code."]);
        return "ERR no qr".to_string();
    }

    let _ = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/plain", &result, &["--sensitive"]);
    let _ = run(&notify(), &["-a", "capture", "-G", "f0c5", "QR code copied", "The decoded value is on the clipboard."]);
    "OK".to_string()
}

fn release_freeze(path: &std::path::Path) {
    if let Ok(pid) = std::fs::read_to_string(path) {
        let pid = pid.trim();
        if !pid.is_empty() {
            let _ = run("kill", &[pid]);
        }
    }
    let _ = std::fs::remove_file(path);
}

