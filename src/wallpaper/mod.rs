//! Wallpaper: set/watch and library.
//!
//! Split from a single 560-line wallpaper.rs for readability:
//! - `state` — state dir, current theme, legacy delegate, paths
//! - `library` — scan/fuzzy/thumbs/search
//! - `playback` — pause/resume/status, set, watch
//!
//! `handle()` below is just the dispatcher over those.

pub mod state;
pub mod library;
pub mod playback;

use state::{current_theme, delegate, state_dir};
use std::fs;
use std::path::PathBuf;

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
                vec![std::path::PathBuf::from(&args[1])]
            } else {
                state::watch_dirs()
            };
            library::thumbs(&dirs)
        }
        "search" => {
            if args.len() < 2 { return "ERR usage: wallpaper search QUERY [DIR]".to_string(); }
            let query = &args[1];
            let dirs = if args.len() > 2 {
                vec![PathBuf::from(&args[2])]
            } else {
                state::watch_dirs()
            };
            library::search(&dirs, query)
        }
        "status" => playback::status_video(),
        "set" => {
            if args.len() < 2 { return "ERR usage: wallpaper set FILE".to_string(); }
            playback::set_wallpaper(&args[1])
        }
        "pause" => playback::pause_video(),
        "resume" => playback::resume_video(),
        "toggle-pause" => playback::toggle_pause_video(),
        "watch" => {
            use std::sync::atomic::{AtomicBool, Ordering};
            static WATCHING: AtomicBool = AtomicBool::new(false);
            if WATCHING.swap(true, Ordering::SeqCst) {
                return "already watching".to_string();
            }
            let dirs = if args.len() > 1 { vec![std::path::PathBuf::from(&args[1])] } else { state::watch_dirs() };
            let interval: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
            std::thread::spawn(move || {
                loop {
                    let _ = library::thumbs(&dirs);
                    let files = library::scan(&dirs);
                    let mut entries = Vec::new();
                    for path in files {
                        let tp = library::thumb_path(&state::cache_dir(), &path);
                        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        entries.push(serde_json::json!({
                            "path": path.to_string_lossy(),
                            "thumb": tp.to_string_lossy(),
                            "stem": stem,
                        }));
                    }
                    let index_path = state::cache_dir().join("search_index.json");
                    let _ = fs::create_dir_all(state::cache_dir());
                    if let Ok(json) = serde_json::to_string(&entries) {
                        let _ = fs::write(&index_path, json);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(interval));
                }
            });
            "watching".to_string()
        }
        "watch-foreground" => playback::watch_wallpapers(args),
        "list" | "library" => delegate(args),
        _ => delegate(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn wallpaper_current_parses_wallpapers_json() {
        let _guard = ENV_LOCK.lock().unwrap();
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
