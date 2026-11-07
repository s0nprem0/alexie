use super::color::palette_ok_file;
use super::super::state::{self, generated_file, generations_dir, live_link, mode_file, state_dir, state_ensure, state_set_generation, state_set_theme, write_atomic};
use crate::proc::run;
use std::env;
use std::path::{Path, PathBuf};

fn tool(env_name: &str, default: &str) -> String {
    env::var(env_name).unwrap_or_else(|_| default.to_string())
}

pub(crate) fn theme_apps_bin() -> String {
    if let Ok(v) = env::var("THEME_APPS")
        && !v.is_empty() {
            return v;
        }
    let cfg = crate::config::get_str(&["theme", "theme_apps"], "");
    if !cfg.is_empty() {
        return cfg;
    }
    if let Ok(s) = std::fs::read_to_string(state::live_link().join("renderer"))
        && let Some(path) = s.split_whitespace().nth(1) {
            return path.to_string();
        }
    tool("THEME_APPS", "theme-apps")
}

// ---------- kitty conf (port of write_kitty_conf in core.sh) ----------

pub(crate) fn colour(generated: &serde_json::Value, key: &str) -> String {
    generated.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

pub(crate) fn ansi_colour(generated: &serde_json::Value, index: u8, fallback: &str) -> String {
    let v = generated
        .get(format!("ansi{}", index))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    v.unwrap_or_else(|| colour(generated, fallback))
}

pub(crate) fn write_kitty(generated: &serde_json::Value, dest: &Path) -> Result<(), String> {
    let c = |k: &str| colour(generated, k);
    let a = |i: u8, f: &str| ansi_colour(generated, i, f);
    let content = format!(
        "background {}\nforeground {}\nselection_background {}\nselection_foreground {}\ncolor0 {}\ncolor8 {}\ncolor1 {}\ncolor9 {}\ncolor2 {}\ncolor10 {}\ncolor3 {}\ncolor11 {}\ncolor4 {}\ncolor12 {}\ncolor5 {}\ncolor13 {}\ncolor6 {}\ncolor14 {}\ncolor7 {}\ncolor15 {}\n",
        c("background"),
        c("foreground"),
        c("accent"),
        c("background"),
        a(0, "background"),
        a(8, "muted"),
        a(1, "danger"),
        a(9, "danger"),
        a(2, "success"),
        a(10, "success"),
        a(3, "yellow"),
        a(11, "yellow"),
        a(4, "primary"),
        a(12, "primary"),
        a(5, "accentAlt"),
        a(13, "accentAlt"),
        a(6, "accent"),
        a(14, "accent"),
        a(7, "foreground"),
        a(15, "foreground"),
    );
    write_atomic(&dest.to_path_buf(), &content).map_err(|e| e.to_string())
}

// ---------- state lock (port of state_lock/state_unlock) ----------

pub(crate) struct StateLock {
    file: std::fs::File,
}

impl StateLock {
    pub(crate) fn acquire() -> Result<Self, String> {
        std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
        // Legacy locks $STATE/.lock with flock(2) (a persistent file
        // that stays behind after unlock). Locking the same file
        // with flock(2) here makes Rust and legacy bash callers
        // mutually exclude each other, instead of a create_new lock
        // file that can never be acquired while .lock exists.
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(state_dir().join(".lock"))
            .map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if ret == 0 {
                return Ok(Self { file });
            }
            if std::time::Instant::now() >= deadline {
                return Err("could not lock the theme state (timeout)".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
        // File is left on disk, like legacy flock users expect.
    }
}

// ---------- prune generations (port of prune_generations) ----------

pub(crate) fn prune_generations(current: &Path) {
    let dir = generations_dir();
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut gens: Vec<(std::time::SystemTime, PathBuf)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let m = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((m, p))
        })
        .collect();
    gens.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut keep = 0;
    for (_, gen_dir) in gens {
        if gen_dir == current {
            continue;
        }
        keep += 1;
        if keep <= 2 {
            continue;
        }
        let _ = std::fs::remove_dir_all(&gen_dir);
    }
}

// ---------- materialise (port of materialize.sh) ----------

pub(crate) fn materialise(name: &str) -> Result<(), String> {
    std::fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(generations_dir()).map_err(|e| e.to_string())?;
    state_ensure()?;

    let mode_value = if name == "wallpaper" {
        if !wallpaper_palette() {
            return Err("the generated palette is missing; run 'theme wallpaper FILE'".to_string());
        }
        std::fs::read_to_string(generated_file())
            .map_err(|e| e.to_string())
            .and_then(|s| {
                serde_json::from_str::<serde_json::Value>(&s)
                    .map_err(|e| e.to_string())
            }).map(|v| v.get("_mode").and_then(|m| m.as_str()).unwrap_or("dark").to_string())?
    } else {
        let entry = super::super::state::palette_entry(name)
            .ok_or_else(|| format!("no such theme: {}", name))?;
        // resolved palette = entry without "_" keys
        let obj = entry.as_object().ok_or_else(|| format!("no such theme: {}", name))?;
        let mut resolved = serde_json::Map::new();
        for (k, v) in obj {
            if !k.starts_with('_') {
                resolved.insert(k.clone(), v.clone());
            }
        }
        let tmp = state_dir().join(format!("palette.{}", std::process::id()));
        let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(resolved))
            .map_err(|e| e.to_string())?;
        write_atomic(&tmp, &format!("{}\n", pretty)).map_err(|e| e.to_string())?;
        if !palette_ok_file(&tmp) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("theme {} is missing colours", name));
        }
        std::fs::rename(&tmp, generated_file()).map_err(|e| e.to_string())?;
        super::super::state::mode_of(name)
    };

    write_atomic(&mode_file(), &format!("{}\n", mode_value)).map_err(|e| e.to_string())?;

    let gen_dir = {
        let stamp = {
            let (code, out, _) = run("date", &["+%Y%m%d%H%M%S"]);
            if code == 0 { out.trim().to_string() } else { "0".to_string() }
        };
        let mut candidate = generations_dir().join(format!("{}-{}", stamp, std::process::id()));
        let mut n = 0;
        while candidate.exists() {
            n += 1;
            candidate = generations_dir().join(format!("{}-{}-{}", stamp, std::process::id(), n));
        }
        std::fs::create_dir_all(&candidate).map_err(|e| e.to_string())?;
        candidate
    };

    let cleanup = |gen_dir: &Path| { let _ = std::fs::remove_dir_all(gen_dir); };
    if std::fs::copy(generated_file(), gen_dir.join("palette.json")).is_err() {
        cleanup(&gen_dir);
        return Err("could not stage the palette".to_string());
    }
    if write_atomic(&gen_dir.join("mode"), &format!("{}\n", mode_value)).is_err() {
        cleanup(&gen_dir);
        return Err("could not stage the mode".to_string());
    }

    let generated_json: serde_json::Value = std::fs::read_to_string(gen_dir.join("palette.json"))
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))?;
    if write_kitty(&generated_json, &gen_dir.join("kitty.conf")).is_err() {
        cleanup(&gen_dir);
        return Err("could not write the terminal palette".to_string());
    }

    // renderer record: embedded palettes + the theme-apps binary in use
    if write_atomic(
        &gen_dir.join("renderer"),
        &format!("embedded-themes {}\n", theme_apps_bin()),
    )
    .is_err()
    {
        cleanup(&gen_dir);
        return Err("could not record the renderer".to_string());
    }

    // Activate the generation BEFORE rendering the slow app files, so the
    // shell can repaint from the palette immediately ("early shell paint").
    // theme-apps still writes into gen_dir, which live_link now points at.
    let tmp_link = state_dir().join(format!(".live.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp_link);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        if symlink(&gen_dir, &tmp_link).is_err() {
            cleanup(&gen_dir);
            return Err("could not activate the generation".to_string());
        }
    }
    #[cfg(not(unix))]
    {
        cleanup(&gen_dir);
        return Err("could not activate the generation".to_string());
    }
    if std::fs::rename(&tmp_link, live_link()).is_err() {
        let _ = std::fs::remove_file(&tmp_link);
        cleanup(&gen_dir);
        return Err("could not activate the generation".to_string());
    }

    // Early shell paint: tell Quickshell the new palette now, before the
    // slow application files (and papirus-folders) are rendered.
    {
        let qs = std::env::var("QUICKSHELL").unwrap_or_else(|_| "quickshell".to_string());
        if name == "wallpaper" {
            let content = std::fs::read_to_string(live_link().join("palette.json"))
                .unwrap_or_default()
                .replace('\n', "");
            let _ = run(&qs, &["ipc", "call", "theme", "palette", &content]);
        } else {
            let _ = run(&qs, &["ipc", "call", "theme", "set", name]);
        }
    }

    // theme-apps renders the application files; it is the one
    // external renderer legacy used (@THEME_APPS@).
    let (code, _, err) = run(&theme_apps_bin(), &[name, gen_dir.to_string_lossy().as_ref()]);
    if code != 0 {
        cleanup(&gen_dir);
        return Err(format!("could not render the application themes: {}", err.trim()));
    }

    for source in [
        "palette.json", "mode", "kitty.conf", "helix.toml", "yazi.toml", "hyprlock.conf",
        "qt6ct-colors.conf", "qt6ct.conf", "qt5ct.conf", "kvantum.kvconfig",
        "gtk-3.0-settings.ini", "gtk-4.0-settings.ini", "gtk-3.0.css", "gtk-4.0.css",
        "share-picker.css", "discord.css", "librewolf-userChrome.css", "librewolf-userContent.css",
        "mpv.conf", "zathurarc", "eza-theme.yml", "opencode-theme.json",
        "tmux.conf", "btop.theme", "terminal.zsh",
    ] {
        if std::fs::metadata(gen_dir.join(source)).map(|m| m.len()).unwrap_or(0) == 0 {
            cleanup(&gen_dir);
            return Err(format!("the generation is missing {}", source));
        }
    }

    let source = if name == "wallpaper" { "wallpaper" } else { "preset" };
    write_atomic(&state::current_file(), &format!("{}\n", name)).map_err(|e| e.to_string())?;
    state_set_theme(name, source)?;
    let gen_name = gen_dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if state_set_generation(&gen_name).is_err() {
        eprintln!("theme: could not record the new generation");
    }

    prune_generations(&gen_dir);
    Ok(())
}

pub(crate) fn wallpaper_palette() -> bool {
    generated_file().exists()
        && palette_ok_file(&generated_file())
        && std::fs::read_to_string(generated_file())
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| v.get("_mode").is_some())
            .unwrap_or(false)
}

