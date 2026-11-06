use crate::proc::{resolve_tool, run, shell_quote};
use std::env;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

fn state_dir() -> PathBuf {
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie")
}

fn current_theme() -> String {
    let p = state_dir().join("current");
    if let Ok(s) = fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    String::new()
}

fn delegate(args: &[String]) -> String {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let tool = resolve_tool("wallpaper");
    let (code, out, err) = run(&tool, &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else {
        format!("ERR (exit {}) {}", code, err.trim())
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn cache_dir() -> PathBuf {
    if let Ok(v) = env::var("WALLPAPER_THUMB_CACHE") {
        return PathBuf::from(v);
    }
    let base = env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{}/.cache", home_dir().display()));
    PathBuf::from(base).join("alexie").join("wallpaper-thumbs")
}

fn watch_dirs() -> Vec<PathBuf> {
    if let Ok(v) = env::var("WALLPAPER_WATCH_DIRS") {
        return v.split(':').filter(|p| !p.trim().is_empty()).map(PathBuf::from).collect();
    }
    if let Ok(v) = env::var("WALLPAPER_DIR") {
        return vec![PathBuf::from(v)];
    }
    if let Some(list) = crate::config::get_str_list(&["wallpaper", "dirs"]) {
        if !list.is_empty() {
            return list.into_iter().map(PathBuf::from).collect();
        }
    }
    vec![home_dir().join("Pictures").join("Wallpapers")]
}

fn is_supported(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|e| {
        matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp")
    }).unwrap_or(false)
}

fn stable_hash(path: &Path) -> String {
    let mut h = DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn thumb_path(cache: &Path, path: &Path) -> PathBuf {
    cache.join(format!("{}.jpg", stable_hash(path)))
}

fn scan(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue; };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && is_supported(&p) {
                out.push(p.clone());
            }
            // one level of recursion for subdirs (common for wallpaper packs)
            if p.is_dir() {
                if let Ok(sub) = fs::read_dir(&p) {
                    for e in sub.flatten() {
                        let sp = e.path();
                        if sp.is_file() && is_supported(&sp) { out.push(sp); }
                    }
                }
            }
        }
    }
    out
}

fn fuzzy_score(query: &str, text: &str) -> i64 {
    let q = query.to_lowercase();
    let t = text.to_lowercase();
    if t == q { return 1000; }
    if t.starts_with(&q) { return 800 + (100 - t.len().min(100) as i64); }
    if t.contains(&q) { return 600 + (100 - t.len().min(100) as i64); }
    let q_chars: Vec<char> = q.chars().collect();
    let t_chars: Vec<char> = t.chars().collect();
    let mut qi = 0usize;
    let mut score: i64 = 0;
    let mut last: Option<usize> = None;
    for (ti, &tc) in t_chars.iter().enumerate() {
        if qi < q_chars.len() && tc == q_chars[qi] {
            score += 10;
            if last.is_some_and(|lm| lm + 1 == ti) { score += 15; }
            last = Some(ti);
            qi += 1;
        }
    }
    if qi == q_chars.len() {
        score += 200;
        let coverage = q.len() as f64 / t.len().max(1) as f64;
        score += (coverage * 100.0) as i64;
        score
    } else {
        -1
    }
}

fn thumbs(dirs: &[PathBuf]) -> String {
    let cache = cache_dir();
    let _ = fs::create_dir_all(&cache);
    let files = scan(dirs);
    let mut entries = Vec::new();
    for path in files {
        let tp = thumb_path(&cache, &path);
        // regenerate if missing or stale
        let needs = match fs::metadata(&tp) {
            Ok(m) => m.len() == 0 || m.modified().map(|mt| {
                fs::metadata(&path).ok().and_then(|pm| pm.modified().ok()).map(|pt| pt > mt).unwrap_or(true)
            }).unwrap_or(true),
            Err(_) => true,
        };
        if needs {
            let _ = Command::new("magick")
                .args([path.to_str().unwrap_or(""), "-resize", "440x248^", "-gravity", "center", "-extent", "440x248", "-quality", "85", tp.to_str().unwrap_or("")])
                .status();
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        entries.push(serde_json::json!({
            "path": path.to_string_lossy(),
            "thumb": tp.to_string_lossy(),
            "stem": stem,
        }));
    }
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

fn search(dirs: &[PathBuf], query: &str) -> String {
    let files = scan(dirs);
    let mut scored: Vec<(i64, PathBuf)> = files.into_iter().filter_map(|p| {
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let s = fuzzy_score(query, &stem);
        if s >= 0 { Some((s, p)) } else { None }
    }).collect();
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    let paths: Vec<String> = scored.into_iter().map(|(_, p)| p.to_string_lossy().to_string()).collect();
    serde_json::to_string(&paths).unwrap_or_else(|_| "[]".to_string())
}

fn pause_toggle() -> String {
    use std::io::Write;
    let paused_flag = cache_dir().join("live_wallpaper_paused");
    let osdctl = format!("{}/.config/quickshell/osd/bin/osdctl", home_dir().display());
    let socket_path = "/tmp/mpvpaper-ipc";
    let mut stream = match std::os::unix::net::UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(_) => {
            let _ = Command::new(&osdctl).args(["show", "no live wallpaper", "info", "1200"]).status();
            return "no live wallpaper".to_string();
        }
    };
    if let Some(parent) = paused_flag.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let is_paused = fs::read_to_string(&paused_flag).map(|s| s.trim() == "true").unwrap_or(false);
    if is_paused {
        let _ = fs::write(&paused_flag, "false");
        let _ = writeln!(stream, r#"{{"command": ["set_property", "pause", false]}}"#);
        let _ = Command::new(&osdctl).args(["show", "live wallpaper resumed", "good", "1200"]).status();
        "resumed".to_string()
    } else {
        let _ = fs::write(&paused_flag, "true");
        let _ = writeln!(stream, r#"{{"command": ["set_property", "pause", true]}}"#);
        let _ = Command::new(&osdctl).args(["show", "live wallpaper paused", "warn", "1200"]).status();
        "paused".to_string()
    }
}

fn set_wallpaper(raw: &str) -> String {
    let path = match std::fs::canonicalize(raw) {
        Ok(p) => p,
        Err(e) => return format!("ERR cannot resolve path '{}': {}", raw, e),
    };
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
    let path_str = path.to_string_lossy().to_string();
    if ext == "mp4" || ext == "webm" {
        // kill existing mpvpaper
        let _ = Command::new("pkill").args(["-f", "mpvpaper"]).status();
        for _ in 0..20 {
            let running = Command::new("pgrep").args(["-f", "mpvpaper"]).status().map(|s| s.success()).unwrap_or(false);
            if !running { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let paused_flag = cache_dir().join("live_wallpaper_paused");
        let is_paused = fs::read_to_string(&paused_flag).map(|s| s.trim() == "true").unwrap_or(false);
        let mut mpv_opts = String::from("--loop --no-audio --hwdec=no --load-scripts=no --cache=no --demuxer-max-bytes=10M --vd-lavc-fast=yes --vd-lavc-skiploopfilter=all --vf=fps=30 --scale=bilinear --cscale=bilinear --dscale=bilinear --sws-scaler=fast-bilinear --correct-downscaling=no --linear-downscaling=no --sigmoid-upscaling=no --hdr-compute-peak=no --input-ipc-server=/tmp/mpvpaper-ipc");
        if is_paused { mpv_opts.push_str(" --pause"); }
        let cmd = format!("uwsm app -- mpvpaper -o \"{}\" '*' '{}' >/tmp/mpvpaper_rust.log 2>&1", mpv_opts, path_str);
        match Command::new("bash").args(["-c", &cmd]).spawn() {
            Ok(_) => format!("wallpaper set to {}", path_str),
            Err(e) => format!("ERR spawn mpvpaper: {}", e),
        }
    } else {
        // image: try swww
        let swww = Command::new("sh").args(["-c", "command -v swww"]).output();
        let has_swww = swww.map(|o| o.status.success()).unwrap_or(false);
        if has_swww {
            let status = Command::new("swww").args(["img", &path_str]).status();
            match status {
                Ok(s) if s.success() => format!("wallpaper set to {}", path_str),
                Ok(s) => format!("ERR swww exited {}", s.code().unwrap_or(-1)),
                Err(e) => format!("ERR swww: {}", e),
            }
        } else {
            // fallback to legacy wallpaper binary
            let a = ["set", &path_str];
            let (code, out, err) = crate::proc::run("wallpaper", &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } } else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}

fn watch_wallpapers(args: &[String]) -> ! {
    let dirs = if args.len() > 1 { vec![PathBuf::from(&args[1])] } else { watch_dirs() };
    let interval: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    // Run in foreground; generate thumbnails and search index.
    loop {
        let _ = thumbs(&dirs); // ensure thumbs generated
        // build search index
        let files = scan(&dirs);
        let mut entries = Vec::new();
        for path in files {
            let tp = thumb_path(&cache_dir(), &path);
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            entries.push(serde_json::json!({
                "path": path.to_string_lossy(),
                "thumb": tp.to_string_lossy(),
                "stem": stem,
            }));
        }
        let index_path = cache_dir().join("search_index.json");
        let _ = fs::create_dir_all(cache_dir());
        if let Ok(json) = serde_json::to_string(&entries) {
            let _ = fs::write(&index_path, json);
        }
        std::thread::sleep(std::time::Duration::from_secs(interval));
    }
}

pub fn handle(args: &[String]) -> String {
    if args.is_empty() {
        return delegate(args);
    }
    match args[0].as_str() {
        "current" => {
            let theme = current_theme();
            let file = state_dir().join(format!("wallpaper.{}", theme));
            if let Ok(s) = fs::read_to_string(&file) {
                let s = s.trim().to_string();
                if !s.is_empty() { return s; }
            }
            let wp = state_dir().join("wallpapers.json");
            if let Ok(s) = fs::read_to_string(&wp) {
                let filter = format!(r#".themes["{}"].fallback.path // empty"#, theme);
                let script = format!("printf '%s' {} | jq -r '{}'", shell_quote(&s), filter);
                let (code, out, _) = run("sh", &["-c", &script]);
                if code == 0 {
                    let out = out.trim();
                    if !out.is_empty() && out != "null" { return out.to_string(); }
                }
            }
            "generated".to_string()
        }
        "thumbs" => {
            let dirs = if args.len() > 1 {
                vec![PathBuf::from(&args[1])]
            } else {
                watch_dirs()
            };
            thumbs(&dirs)
        }
        "search" => {
            if args.len() < 2 { return "ERR usage: wallpaper search QUERY [DIR]".to_string(); }
            let query = &args[1];
            let dirs = if args.len() > 2 {
                vec![PathBuf::from(&args[2])]
            } else {
                watch_dirs()
            };
            search(&dirs, query)
        }
        "status" => delegate(args),
        "set" => {
            if args.len() < 2 { return "ERR usage: wallpaper set FILE".to_string(); }
            set_wallpaper(&args[1])
        }
        "pause" | "toggle-pause" | "resume" => pause_toggle(),
        "watch" => watch_wallpapers(args),
        "list" | "library" => delegate(args),
        _ => delegate(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_basic() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn current_theme_no_panic() {
        let _ = current_theme();
    }

    #[test]
    fn fuzzy_score_basic() {
        assert!(fuzzy_score("foo", "foobar") > 0);
        assert_eq!(fuzzy_score("zzz", "foobar"), -1);
    }
}
