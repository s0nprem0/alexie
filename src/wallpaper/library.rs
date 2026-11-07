use super::state::cache_dir;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn stable_hash(path: &Path) -> String {
    let mut h = DefaultHasher::new();
    path.to_string_lossy().hash(&mut h);
    format!("{:016x}", h.finish())
}

pub(crate) fn thumb_path(cache: &Path, path: &Path) -> PathBuf {
    cache.join(format!("{}.jpg", stable_hash(path)))
}

pub(crate) fn scan(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && is_supported(&p) {
                out.push(p.clone());
            }
            // one level of recursion for subdirs (common for wallpaper packs)
            if p.is_dir()
                && let Ok(sub) = fs::read_dir(&p)
            {
                for e in sub.flatten() {
                    let sp = e.path();
                    if sp.is_file() && is_supported(&sp) {
                        out.push(sp);
                    }
                }
            }
        }
    }
    out
}

pub(crate) fn fuzzy_score(query: &str, text: &str) -> i64 {
    let q = query.to_lowercase();
    let t = text.to_lowercase();
    if t == q {
        return 1000;
    }
    if t.starts_with(&q) {
        return 800 + (100 - t.len().min(100) as i64);
    }
    if t.contains(&q) {
        return 600 + (100 - t.len().min(100) as i64);
    }
    let q_chars: Vec<char> = q.chars().collect();
    let t_chars: Vec<char> = t.chars().collect();
    let mut qi = 0usize;
    let mut score: i64 = 0;
    let mut last: Option<usize> = None;
    for (ti, &tc) in t_chars.iter().enumerate() {
        if qi < q_chars.len() && tc == q_chars[qi] {
            score += 10;
            if last.is_some_and(|lm| lm + 1 == ti) {
                score += 15;
            }
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

pub(crate) fn thumbs(dirs: &[PathBuf]) -> String {
    let cache = cache_dir();
    let _ = fs::create_dir_all(&cache);
    let files = scan(dirs);
    let mut entries = Vec::new();
    for path in files {
        let tp = thumb_path(&cache, &path);
        // regenerate if missing or stale
        let needs = match fs::metadata(&tp) {
            Ok(m) => {
                m.len() == 0
                    || m.modified()
                        .map(|mt| {
                            fs::metadata(&path)
                                .ok()
                                .and_then(|pm| pm.modified().ok())
                                .map(|pt| pt > mt)
                                .unwrap_or(true)
                        })
                        .unwrap_or(true)
            }
            Err(_) => true,
        };
        if needs {
            let _ = Command::new("magick")
                .args([
                    path.to_str().unwrap_or(""),
                    "-resize",
                    "440x248^",
                    "-gravity",
                    "center",
                    "-extent",
                    "440x248",
                    "-quality",
                    "85",
                    tp.to_str().unwrap_or(""),
                ])
                .status();
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        entries.push(serde_json::json!({
            "path": path.to_string_lossy(),
            "thumb": tp.to_string_lossy(),
            "stem": stem,
        }));
    }
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

pub(crate) fn search(dirs: &[PathBuf], query: &str) -> String {
    let files = scan(dirs);
    let mut scored: Vec<(i64, PathBuf)> = files
        .into_iter()
        .filter_map(|p| {
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let s = fuzzy_score(query, &stem);
            if s >= 0 { Some((s, p)) } else { None }
        })
        .collect();
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    let paths: Vec<String> = scored
        .into_iter()
        .map(|(_, p)| p.to_string_lossy().to_string())
        .collect();
    serde_json::to_string(&paths).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_score_basic() {
        assert!(fuzzy_score("foo", "foobar") > 0);
        assert_eq!(fuzzy_score("zzz", "foobar"), -1);
    }
}
