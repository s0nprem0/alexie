use crate::proc::run;
use std::env;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

fn state_dir() -> PathBuf {
    // Legacy wallpaper/theme state is $XDG_STATE_HOME/anixie/theme
    // (anixie.theme.stateDir), e.g. wallpapers.json, wallpaper.$theme
    // and video-paused all live there.
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie").join("theme")
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
    // Delegate heavy ops to the legacy bash implementation explicitly,
    // like theme/shot/record. resolve_tool("wallpaper") can re-enter
    // alexie via the wallpaper shim when daemon mode is on and the
    // legacy binary has been removed.
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, out, err) = run("wallpaper-legacy", &a);
    if code == 0 {
        if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
    } else if code == -1 {
        format!(
            "ERR wallpaper-legacy not found: subcommand `{}` needs the legacy bash wallpaper (enable anixie.daemon, which renames it to wallpaper-legacy)",
            args.first().map(|s| s.as_str()).unwrap_or("")
        )
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

fn video_paused_file() -> PathBuf {
    // Matches legacy wallpaper.sh ($STATE/video-paused) and the
    // Appearance panel's FileView, which watches this exact path.
    state_dir().join("video-paused")
}

fn is_video_paused() -> bool {
    video_paused_file().exists()
}

/// Pure status logic, mirroring legacy `video_status()`:
/// not running → stopped, running+flag → paused, else running.
fn video_status(running: bool, paused: bool) -> &'static str {
    if !running {
        "stopped"
    } else if paused {
        "paused"
    } else {
        "running"
    }
}

fn mpvpaper_running() -> bool {
    // pgrep -x matches the exact process name, like legacy $PGREP.
    let (code, _, _) = run("pgrep", &["-x", "mpvpaper"]);
    code == 0
}

fn pause_video() -> String {
    // Legacy: pkill -STOP -x mpvpaper; write $STATE/video-paused
    let _ = run("pkill", &["-STOP", "-x", "mpvpaper"]);
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(video_paused_file(), "1\n");
    "paused".to_string()
}

fn resume_video() -> String {
    // Legacy: pkill -CONT -x mpvpaper; remove $STATE/video-paused.
    // Idempotent: resuming when not paused stays resumed, it does
    // not toggle back to paused (the old pause_toggle did).
    let _ = run("pkill", &["-CONT", "-x", "mpvpaper"]);
    let _ = fs::remove_file(video_paused_file());
    "resumed".to_string()
}

fn toggle_pause_video() -> String {
    if is_video_paused() {
        resume_video()
    } else {
        pause_video()
    }
}

fn status_video() -> String {
    video_status(mpvpaper_running(), is_video_paused()).to_string()
}

fn set_wallpaper(raw: &str) -> String {
    let path = match std::fs::canonicalize(raw) {
        Ok(p) => p,
        Err(e) => return format!("ERR cannot resolve path '{}': {}", raw, e),
    };
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
    let path_str = path.to_string_lossy().to_string();
    if ext == "mp4" || ext == "webm" {
        // Kill existing mpvpaper by exact name, like legacy apply().
        let _ = run("pkill", &["-x", "mpvpaper"]);
        for _ in 0..20 {
            if !mpvpaper_running() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut mpv_opts = String::from("--loop --no-audio --hwdec=no --load-scripts=no --cache=no --demuxer-max-bytes=10M --vd-lavc-fast=yes --vd-lavc-skiploopfilter=all --vf=fps=30 --scale=bilinear --cscale=bilinear --dscale=bilinear --sws-scaler=fast-bilinear --correct-downscaling=no --linear-downscaling=no --sigmoid-upscaling=no --hdr-compute-peak=no --input-ipc-server=/tmp/mpvpaper-ipc");
        if is_video_paused() { mpv_opts.push_str(" --pause"); }
        // Spawn directly with argv instead of bash -c with the path
        // interpolated into a shell string (breaks / injects on paths
        // with spaces, quotes or $).
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(cache_dir().join("mpvpaper.log"));
        let mut cmd = Command::new("uwsm");
        cmd.args(["app", "--", "mpvpaper", "-o", &mpv_opts, "*", &path_str]);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        if let Ok(f) = log {
            cmd.stderr(f);
        } else {
            cmd.stderr(std::process::Stdio::null());
        }
        match cmd.spawn() {
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
            // fallback to legacy wallpaper binary explicitly, not
            // `wallpaper` (which is the alexie shim when daemon is on)
            let (code, out, err) = crate::proc::run("wallpaper-legacy", &["set", &path_str]);
            if code == 0 {
                if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() }
            } else if code == -1 {
                "ERR wallpaper-legacy not found: set needs the legacy bash wallpaper (enable anixie.daemon, which renames it to wallpaper-legacy)".to_string()
            } else {
                format!("ERR (exit {}) {}", code, err.trim())
            }
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
            // Match legacy chosen(): wallpapers.json fallback first,
            // then wallpaper.$theme file, else "generated".
            // Parse with serde_json instead of a jq shell-out whose
            // filter interpolates the theme name unsanitised.
            let theme = current_theme();
            let wp = state_dir().join("wallpapers.json");
            if let Ok(s) = fs::read_to_string(&wp) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&s) {
                    if let Some(path) = value
                        .get("themes")
                        .and_then(|t| t.get(&theme))
                        .and_then(|t| t.get("fallback"))
                        .and_then(|f| f.get("path"))
                        .and_then(|p| p.as_str())
                    {
                        if !path.is_empty() {
                            return path.to_string();
                        }
                    }
                }
            }
            let file = state_dir().join(format!("wallpaper.{}", theme));
            if let Ok(s) = fs::read_to_string(&file) {
                let s = s.trim().to_string();
                if !s.is_empty() { return s; }
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
        "status" => status_video(),
        "set" => {
            if args.len() < 2 { return "ERR usage: wallpaper set FILE".to_string(); }
            set_wallpaper(&args[1])
        }
        "pause" => pause_video(),
        "resume" => resume_video(),
        "toggle-pause" => toggle_pause_video(),
        "watch" => {
            use std::sync::atomic::{AtomicBool, Ordering};
            static WATCHING: AtomicBool = AtomicBool::new(false);
            if WATCHING.swap(true, Ordering::SeqCst) {
                return "already watching".to_string();
            }
            let dirs = if args.len() > 1 { vec![PathBuf::from(&args[1])] } else { watch_dirs() };
            let interval: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
            std::thread::spawn(move || {
                loop {
                    let _ = thumbs(&dirs);
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
            });
            "watching".to_string()
        }
        "watch-foreground" => watch_wallpapers(args),
        "list" | "library" => delegate(args),
        _ => delegate(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_theme_no_panic() {
        let _ = current_theme();
    }

    #[test]
    fn fuzzy_score_basic() {
        assert!(fuzzy_score("foo", "foobar") > 0);
        assert_eq!(fuzzy_score("zzz", "foobar"), -1);
    }

    #[test]
    fn state_dir_is_anixie_theme() {
        let dir = state_dir();
        assert!(
            dir.ends_with(std::path::Path::new("anixie").join("theme")),
            "got: {}",
            dir.display()
        );
    }

    #[test]
    fn video_status_logic() {
        // Mirrors legacy video_status(): stopped / paused / running,
        // and is not a toggle.
        assert_eq!(video_status(false, false), "stopped");
        assert_eq!(video_status(false, true), "stopped");
        assert_eq!(video_status(true, true), "paused");
        assert_eq!(video_status(true, false), "running");
    }

    #[test]
    fn pause_resume_are_idempotent_not_toggle() {
        // Regression: the old pause_toggle() flipped on every call,
        // so `resume` when not paused would pause, and `pause`
        // twice would resume. Exercise the file flag via handle()
        // with a temporary XDG_STATE_HOME.
        let tmp = std::env::temp_dir().join(format!(
            "alexie-wallpaper-pause-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let old = std::env::var("XDG_STATE_HOME").ok();
        std::env::set_var("XDG_STATE_HOME", &tmp);

        let flag = tmp.join("anixie").join("theme").join("video-paused");
        // resume when nothing is paused: stays resumed, no flag
        assert_eq!(handle(&["resume".to_string()]), "resumed");
        assert!(!flag.exists());
        // pause twice: still paused
        assert_eq!(handle(&["pause".to_string()]), "paused");
        assert!(flag.exists());
        assert_eq!(handle(&["pause".to_string()]), "paused");
        assert!(flag.exists());
        // resume twice: still resumed
        assert_eq!(handle(&["resume".to_string()]), "resumed");
        assert!(!flag.exists());
        assert_eq!(handle(&["resume".to_string()]), "resumed");
        assert!(!flag.exists());
        // toggle-pause flips based on the flag
        assert_eq!(handle(&["toggle-pause".to_string()]), "paused");
        assert_eq!(handle(&["toggle-pause".to_string()]), "resumed");

        match old {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn status_is_one_of_legacy_values() {
        let out = handle(&["status".to_string()]);
        assert!(
            matches!(out.as_str(), "stopped" | "paused" | "running"),
            "got: {out}"
        );
    }

    #[test]
    fn delegate_unknown_fails_fast() {
        let out = delegate(&["__bogus_subcommand__".to_string()]);
        assert!(out.starts_with("ERR"), "got: {out}");
    }

    #[test]
    fn wallpaper_current_parses_wallpapers_json() {
        // Exercises the serde_json path in handle("current") without
        // a jq shell-out: with a temp state dir containing a theme
        // and wallpapers.json, current returns the fallback path.
        let tmp = std::env::temp_dir().join(format!(
            "alexie-wallpaper-current-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let theme_dir = tmp.join("anixie").join("theme");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&theme_dir).unwrap();
        std::fs::write(theme_dir.join("current"), "mytheme\n").unwrap();
        std::fs::write(
            theme_dir.join("wallpapers.json"),
            r#"{"version":1,"themes":{"mytheme":{"fallback":{"mode":"file","path":"/tmp/wp.png","video":false},"outputs":{}}}}"#,
        )
        .unwrap();
        let old = std::env::var("XDG_STATE_HOME").ok();
        std::env::set_var("XDG_STATE_HOME", &tmp);
        let out = handle(&["current".to_string()]);
        match old {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(out, "/tmp/wp.png");
    }
}
