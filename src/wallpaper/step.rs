//! Stepping, rotation and the picker.
//!
//! Port of step()/set_rotation()/pick_one() in legacy wallpaper.sh.

use super::apply::chosen;
use super::catalog::{library_paths, rotation_file, thumb_for};
use super::select::use_wallpaper;
use super::state::current_theme;
use std::path::Path;

/// Pick the next entry after `here` in the library list, wrapping.
/// Pure, so it can be tested without a filesystem.
pub(crate) fn step_target(list: &[String], here: &str, direction: &str, random: bool, salt: u64) -> Option<String> {
    if list.is_empty() {
        return None;
    }
    if random {
        let candidates: Vec<&String> = list.iter().filter(|p| p.as_str() != here).collect();
        if candidates.is_empty() {
            return None;
        }
        let idx = (salt % candidates.len() as u64) as usize;
        return Some(candidates[idx].clone());
    }
    let len = list.len();
    let at = list.iter().position(|p| p == here).map(|i| i + 1).unwrap_or(1);
    let next = if direction == "next" {
        (at % len) + 1
    } else {
        ((at + len - 2) % len) + 1
    };
    Some(list[next - 1].clone())
}

fn salt() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 ^ (std::process::id() as u64) << 32)
        .unwrap_or(0)
}

/// `next`/`prev`/`rotate`: move to another library entry and use it.
pub(crate) fn step(direction: &str, arg: Option<&str>) -> String {
    let random = matches!(arg, Some("--random") | Some("random"));
    let list = library_paths();
    let here = chosen();
    match step_target(&list, &here, direction, random, salt()) {
        Some(target) => use_wallpaper(&target, ""),
        None => "OK".to_string(),
    }
}

/// Minutes between rotations; 0 turns it off. The file is the
/// source of truth (the shell owns the timer).
pub(crate) fn rotation_minutes() -> i64 {
    std::fs::read_to_string(rotation_file())
        .map(|s| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<i64>().unwrap_or(0))
        .unwrap_or(0)
}

pub(crate) fn set_rotation(value: &str) -> String {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
        return "ERR wallpaper: rotation needs whole minutes (0 turns it off)".to_string();
    }
    let minutes: i64 = value.parse().unwrap_or(-1);
    if !(0..=1440).contains(&minutes) {
        return "ERR wallpaper: rotation must be at most 1440 minutes".to_string();
    }
    match crate::theme::state::write_atomic(&rotation_file(), &format!("{}\n", minutes)) {
        Ok(()) => "OK".to_string(),
        Err(e) => format!("ERR {}", e),
    }
}

/// The bare `wallpaper` picker: generated first, then every library
/// entry, handed to `pick` which runs `wallpaper use <choice>`.
pub(crate) fn pick_one() -> String {
    let theme = current_theme();
    let mut lines = Vec::new();
    let generated_thumb = thumb_for(&super::state::generated_file(""), "", "");
    lines.push(format!(
        "Generated (from the {} palette)\tgenerated\t\t\t{}",
        theme, generated_thumb
    ));
    for path in library_paths() {
        if path == "generated" {
            continue;
        }
        let name = Path::new(&path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
        let thumb = thumb_for(Path::new(&path), "", "");
        lines.push(format!("{}\t{}\t\t\t{}", name, path, thumb));
    }
    let data = lines.join("\n") + "\n";
    let title = format!("wallpaper — {}", theme);
    crate::pick::handle_with_stdin(&["--title".to_string(), title, "--on-select".to_string(), "wallpaper use '{}'".to_string()], &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<String> {
        vec!["generated".to_string(), "/pics/a.jpg".to_string(), "/pics/b.jpg".to_string()]
    }

    #[test]
    fn step_wraps_both_ways() {
        assert_eq!(step_target(&list(), "generated", "next", false, 0), Some("/pics/a.jpg".to_string()));
        assert_eq!(step_target(&list(), "/pics/b.jpg", "next", false, 0), Some("generated".to_string()));
        assert_eq!(step_target(&list(), "generated", "prev", false, 0), Some("/pics/b.jpg".to_string()));
        assert_eq!(step_target(&list(), "/nope", "next", false, 0), Some("/pics/a.jpg".to_string()));
        assert_eq!(step_target(&[], "generated", "next", false, 0), None);
    }

    #[test]
    fn step_random_excludes_current() {
        for salt in 0..20 {
            let target = step_target(&list(), "/pics/a.jpg", "next", true, salt).unwrap();
            assert_ne!(target, "/pics/a.jpg");
        }
        assert_eq!(step_target(&["only".to_string()], "only", "next", true, 0), None);
    }
}
