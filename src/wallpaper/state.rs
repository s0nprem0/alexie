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

pub(crate) fn data_dir() -> PathBuf {
    let base = env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/share", home)
    });
    PathBuf::from(base).join("anixie")
}

/// Per-theme backgrounds folder: $DATA/backgrounds/<theme>.
pub(crate) fn theme_dir() -> PathBuf {
    data_dir().join("backgrounds").join(current_theme())
}

/// The configured default wallpaper, empty when unset (the legacy
/// build bakes this in as @DEFAULT_WALLPAPER@).
pub(crate) fn default_wallpaper() -> String {
    if let Ok(v) = env::var("DEFAULT_WALLPAPER") {
        return v;
    }
    crate::config::get_str(&["wallpaper", "default"], "")
}

pub(crate) fn wallpaper_recursive() -> bool {
    match env::var("WALLPAPER_RECURSIVE") {
        Ok(v) => v == "true",
        Err(_) => crate::config::get_str(&["wallpaper", "recursive"], "false") == "true",
    }
}

pub(crate) fn thumbs_dir() -> PathBuf { state_dir().join("thumbs") }
pub(crate) fn posters_dir() -> PathBuf { state_dir().join("posters") }
pub(crate) fn generated_dir() -> PathBuf { state_dir().join("wallpapers") }

/// The palette-gradient wallpaper file for a theme and optional
/// output: `<theme>-wallpaper.png`, or `<theme>-<output>.png` with
/// the output name sanitised. The theme-specific name makes the path
/// change on theme switch, which is how the shell keys its reload.
pub(crate) fn generated_file(output: &str) -> PathBuf {
    let theme = current_theme();
    if output.is_empty() {
        generated_dir().join(format!("{}-wallpaper.png", theme))
    } else {
        let safe: String = output
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
            .collect();
        generated_dir().join(format!("{}-{}.png", theme, safe))
    }
}

pub(crate) fn watch_dirs() -> Vec<PathBuf> {
    if let Ok(v) = env::var("WALLPAPER_WATCH_DIRS") {
        return v.split(':').filter(|p| !p.trim().is_empty()).map(PathBuf::from).collect();
    }
    if let Ok(v) = env::var("WALLPAPER_DIR") {
        return vec![PathBuf::from(v)];
    }
    if let Some(list) = crate::config::get_str_list(&["wallpaper", "dirs"])
        && !list.is_empty() {
            return list.into_iter().map(PathBuf::from).collect();
        }
    // Legacy defaults (anixie.theme.wallpaper.dirs).
    vec![
        home_dir().join("Pictures").join("Wallpapers"),
        home_dir().join("Pictures"),
        data_dir().join("backgrounds"),
    ]
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

}
