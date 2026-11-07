use super::state::{cache_dir, state_dir, watch_dirs};
use crate::proc::run;
use std::fs;
use std::path::PathBuf;

pub(crate) fn video_paused_file() -> PathBuf {
    // Matches legacy wallpaper.sh ($STATE/video-paused) and the
    // Appearance panel's FileView, which watches this exact path.
    state_dir().join("video-paused")
}

pub(crate) fn is_video_paused() -> bool {
    video_paused_file().exists()
}

/// Pure status logic, mirroring legacy `video_status()`:
/// not running → stopped, running+flag → paused, else running.
pub(crate) fn video_status(running: bool, paused: bool) -> &'static str {
    if !running {
        "stopped"
    } else if paused {
        "paused"
    } else {
        "running"
    }
}

pub(crate) fn mpvpaper_running() -> bool {
    // pgrep -x matches the exact process name, like legacy $PGREP.
    let (code, _, _) = run("pgrep", &["-x", "mpvpaper"]);
    code == 0
}

pub(crate) fn pause_video() -> String {
    // Legacy: pkill -STOP -x mpvpaper; write $STATE/video-paused
    let _ = run("pkill", &["-STOP", "-x", "mpvpaper"]);
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(video_paused_file(), "1\n");
    "paused".to_string()
}

pub(crate) fn resume_video() -> String {
    // Legacy: pkill -CONT -x mpvpaper; remove $STATE/video-paused.
    // Idempotent: resuming when not paused stays resumed, it does
    // not toggle back to paused (the old pause_toggle did).
    let _ = run("pkill", &["-CONT", "-x", "mpvpaper"]);
    let _ = fs::remove_file(video_paused_file());
    "resumed".to_string()
}

pub(crate) fn toggle_pause_video() -> String {
    if is_video_paused() {
        resume_video()
    } else {
        pause_video()
    }
}

pub(crate) fn status_video() -> String {
    video_status(mpvpaper_running(), is_video_paused()).to_string()
}

pub(crate) fn watch_wallpapers(args: &[String]) -> ! {
    let dirs = if args.len() > 1 { vec![PathBuf::from(&args[1])] } else { watch_dirs() };
    let interval: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    // Run in foreground; generate thumbnails and search index.
    loop {
        let _ = super::library::thumbs(&dirs); // ensure thumbs generated
        // build search index
        let files = super::library::scan(&dirs);
        let mut entries = Vec::new();
        for path in files {
            let tp = super::library::thumb_path(&cache_dir(), &path);
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallpaper::state::ENV_LOCK;

    #[test]
    fn video_status_logic() {
        assert_eq!(video_status(false, false), "stopped");
        assert_eq!(video_status(false, true), "stopped");
        assert_eq!(video_status(true, true), "paused");
        assert_eq!(video_status(true, false), "running");
    }

    #[test]
    fn pause_resume_are_idempotent_not_toggle() {
        let _guard = ENV_LOCK.lock().unwrap();
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
        unsafe { std::env::set_var("XDG_STATE_HOME", &tmp) }

        let flag = tmp.join("anixie").join("theme").join("video-paused");
        assert_eq!(pause_video(), "paused");
        assert!(flag.exists());
        assert_eq!(pause_video(), "paused");
        assert!(flag.exists());
        assert_eq!(resume_video(), "resumed");
        assert!(!flag.exists());
        assert_eq!(resume_video(), "resumed");
        assert!(!flag.exists());
        assert_eq!(toggle_pause_video(), "paused");
        assert_eq!(toggle_pause_video(), "resumed");

        match old {
            Some(v) => unsafe { std::env::set_var("XDG_STATE_HOME", v) },
            None => unsafe { std::env::remove_var("XDG_STATE_HOME") },
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn status_is_one_of_legacy_values() {
        let out = status_video();
        assert!(
            matches!(out.as_str(), "stopped" | "paused" | "running"),
            "got: {out}"
        );
    }
}
