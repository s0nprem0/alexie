//! Browser/file launching + file index/search + rendering.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use super::db::{add_file_history, open_db};
use super::paths::{cache_dir, fd, fzf, home_dir_str, hyprctl, thunar, xdg_open};
use super::types::{AppsResponse, FileIndexEntry, WebHistoryItem};

pub(crate) fn open_browser(item: &WebHistoryItem) {
    let _ = crate::proc::run(&xdg_open(), &[&item.url]);
    let _ = crate::proc::run(&hyprctl(), &["dispatch", "hl.dsp.focus({workspace=1})"]);
}

// File index/search.

pub(crate) fn index_files() -> String {
    let home = home_dir_str();
    if home.is_empty() {
        return "ERR no HOME".to_string();
    }
    let cache = cache_dir();
    let _ = fs::create_dir_all(&cache);
    let index_path = cache.join("file_index.json");

    // Run fd with cwd=$HOME.
    let output = std::process::Command::new(crate::proc::resolve_tool(&fd()))
        .args([
            "--type", "f", "--hidden",
            "--exclude", ".git", "--exclude", "node_modules",
            "--exclude", ".cache", "--exclude", "target",
            "--max-depth", "8",
        ])
        .current_dir(&home)
        .output();
    let stdout = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return "ERR fd failed (is fd installed?)".to_string(),
    };

    let mut entries: Vec<FileIndexEntry> = Vec::new();
    for line in stdout.lines() {
        let display_path = format!("~/{}", line);
        let name = Path::new(line).file_name().and_then(|f| f.to_str()).unwrap_or(line).to_string();
        entries.push(FileIndexEntry { path: display_path, name });
    }
    if let Ok(serialized) = serde_json::to_string(&entries) {
        let _ = fs::write(&index_path, serialized);
    }
    serde_json::json!({ "indexed": entries.len() }).to_string()
}

pub(crate) fn search_files(query: &str) -> String {
    let cache = cache_dir();
    let index_path = cache.join("file_index.json");
    if !index_path.exists() {
        return "[]".to_string();
    }
    let Ok(content) = fs::read_to_string(&index_path) else { return "[]".to_string() };
    let Ok(entries) = serde_json::from_str::<Vec<FileIndexEntry>>(&content) else { return "[]".to_string() };

    // Prefer fzf, fall back to substring.
    let mut child = std::process::Command::new(crate::proc::resolve_tool(&fzf()))
        .args(["-f", query])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok();

    let filtered: Option<Vec<String>> = {
        let mut fzf_lines: Option<Vec<String>> = None;
        if let Some(c) = child.as_mut() {
            if let Some(stdin) = c.stdin.as_mut() {
                for entry in &entries {
                    let _ = writeln!(stdin, "{}", entry.path);
                }
            }
            if let Some(c2) = child
                && let Ok(output) = c2.wait_with_output()
                    && output.status.success()
                {
                    fzf_lines = Some(String::from_utf8_lossy(&output.stdout).lines().map(|s| s.to_string()).collect());
                }
        }
        fzf_lines
    };

    let selected: Vec<String> = match filtered {
        Some(lines) => lines,
        None => {
            let q = query.to_lowercase();
            entries.iter().map(|e| e.path.clone()).filter(|p| p.to_lowercase().contains(&q)).collect()
        }
    };

    let results: Vec<FileIndexEntry> = selected
        .into_iter()
        .filter_map(|line| entries.iter().find(|e| e.path == line).cloned())
        .take(50)
        .collect();
    serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string())
}

pub(crate) fn open_file(file_path: &str) -> String {
    let home = home_dir_str();
    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
    if let Err(e) = add_file_history(&conn, file_path) {
        eprintln!("Error adding file to history: {}", e);
    }
    let open_path = if let Some(stripped) = file_path.strip_prefix("~/") {
        format!("{}/{}", home, stripped)
    } else {
        file_path.to_string()
    };
    let (code, _, err) = crate::proc::run(&thunar(), &[&open_path]);
    if code == 0 { "OK".to_string() } else { format!("ERR thunar: {}", err.trim()) }
}

// --- rendering ---

pub(crate) fn render_apps(resp: &AppsResponse) -> String {
    if resp.all_apps.is_empty() {
        return "apps: none found".to_string();
    }
    let rows: Vec<Vec<String>> = resp
        .all_apps
        .iter()
        .map(|a| vec![a.name.clone(), a.count.to_string(), a.exec.clone()])
        .collect();
    let mut out = format!("apps ({}):\n", resp.all_apps.len());
    out.push_str(&crate::table::render(&["Name", "Uses", "Exec"], &rows));
    if !resp.bookmarks.is_empty() {
        out.push_str("\n\nbookmarks:\n");
        let rows: Vec<Vec<String>> = resp.bookmarks.iter().map(|b| vec![b.name.clone(), b.url.clone()]).collect();
        out.push_str(&crate::table::render(&["Name", "URL"], &rows));
    }
    out
}
