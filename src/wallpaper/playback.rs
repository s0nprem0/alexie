use super::state::{cache_dir, state_dir, watch_dirs};
use crate::proc::run;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

pub(crate) fn set_wallpaper(raw: &str) -> String {
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
        let has_swww = crate::proc::find_in_path(
            "swww",
            &std::env::var("PATH").unwrap_or_default(),
        )
        .is_some();
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
