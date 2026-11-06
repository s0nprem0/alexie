use crate::proc::run;
use std::env;
use std::fs;
use std::path::PathBuf;

/// Shared lock for tests that mutate XDG_STATE_HOME. Tests in
/// `wallpaper::tests` and `wallpaper::playback::tests` used to
/// declare separate ENV_LOCK statics, so they ran in parallel
/// and stomped on each other's env (pause flag wrote to the
/// wrong tmp dir).
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub(crate) fn state_dir() -> PathBuf {
    // Legacy wallpaper/theme state is $XDG_STATE_HOME/anixie/theme
    // (anixie.theme.stateDir), e.g. wallpapers.json, wallpaper.$theme
    // and video-paused all live there.
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie").join("theme")
}

pub(crate) fn current_theme() -> String {
    let p = state_dir().join("current");
    if let Ok(s) = fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return s; }
    }
    String::new()
}

pub(crate) fn delegate(args: &[String]) -> String {
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

pub(crate) fn home_dir() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub(crate) fn cache_dir() -> PathBuf {
    if let Ok(v) = env::var("WALLPAPER_THUMB_CACHE") {
        return PathBuf::from(v);
    }
    let base = env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{}/.cache", home_dir().display()));
    PathBuf::from(base).join("alexie").join("wallpaper-thumbs")
}

pub(crate) fn watch_dirs() -> Vec<PathBuf> {
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_theme_no_panic() {
        let _ = current_theme();
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
    fn delegate_unknown_fails_fast() {
        let out = delegate(&["__bogus_subcommand__".to_string()]);
        assert!(out.starts_with("ERR"), "got: {out}");
    }
}
