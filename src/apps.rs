//! Apps launcher: desktop apps, usage, bookmarks, history.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub exec: String,
    pub icon: String,
    #[serde(default)]
    pub count: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebHistoryItem {
    pub query: String,
    pub engine: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileHistoryItem {
    pub path: String,
    pub name: String,
    pub timestamp: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileIndexEntry {
    pub path: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BookmarkItem {
    pub url: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct AppsResponse {
    pub most_used: Vec<AppInfo>,
    pub all_apps: Vec<AppInfo>,
    pub web_history: Vec<WebHistoryItem>,
    pub file_history: Vec<FileHistoryItem>,
    pub bookmarks: Vec<BookmarkItem>,
}

fn fd() -> String { env::var("FD").unwrap_or_else(|_| "fd".to_string()) }
fn fzf() -> String { env::var("FZF").unwrap_or_else(|_| "fzf".to_string()) }
fn xdg_open() -> String { env::var("XDG_OPEN").unwrap_or_else(|_| "xdg-open".to_string()) }
fn hyprctl() -> String { env::var("HYPRCTL").unwrap_or_else(|_| "hyprctl".to_string()) }
fn thunar() -> String { env::var("THUNAR").unwrap_or_else(|_| "thunar".to_string()) }

fn home_dir_str() -> String { env::var("HOME").unwrap_or_default() }

pub(crate) fn db_path() -> PathBuf {
    if let Ok(v) = env::var("APPS_DB") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    let state = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/state", home)
    });
    PathBuf::from(state).join("anixie").join("launcher.db")
}

pub(crate) fn cache_dir() -> PathBuf {
    if let Ok(v) = env::var("APPS_CACHE_DIR") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    let base = env::var("XDG_CACHE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.cache", home)
    });
    PathBuf::from(base).join("anixie").join("launcher")
}

pub(crate) fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

pub(crate) fn normalize_url(url: &str) -> String {
    let mut trimmed = url.trim().to_string();
    if !trimmed.contains("://") {
        trimmed = format!("https://{}", trimmed);
    }

    if let Some((scheme, rest)) = trimmed.split_once("://") {
        let scheme = scheme.to_lowercase();
        let (host, path_query) = match rest.split_once('/') {
            Some((h, p)) => (
                h.to_lowercase(),
                if p.is_empty() {
                    String::new()
                } else {
                    format!("/{}", p)
                },
            ),
            None => (rest.to_lowercase(), String::new()),
        };

        let mut path_clean = path_query;
        if path_clean.len() > 1 && path_clean.ends_with('/') {
            path_clean.pop();
        }

        format!("{}://{}{}", scheme, host, path_clean)
    } else {
        trimmed.to_lowercase()
    }
}

pub(crate) fn parse_web_search(query: &str) -> Option<WebHistoryItem> {
    let q = query.trim();
    if !q.starts_with('!') {
        return None;
    }

    let first_space = q.find(' ');
    let (trigger, search_text) = match first_space {
        None => (q.to_lowercase(), ""),
        Some(idx) => (q[..idx].to_lowercase(), q[idx + 1..].trim()),
    };

    let (engine_name, search_url, query_text) = if trigger == "!yt" || trigger == "!youtube" {
        ("youtube".to_string(), "https://www.youtube.com/results?search_query=".to_string(), search_text.to_string())
    } else if trigger == "!g" || trigger == "!google" {
        ("google".to_string(), "https://www.google.com/search?q=".to_string(), search_text.to_string())
    } else if trigger == "!gh" || trigger == "!github" {
        ("github".to_string(), "https://github.com/search?q=".to_string(), search_text.to_string())
    } else if trigger == "!w" || trigger == "!wiki" || trigger == "!wikipedia" {
        ("wikipedia".to_string(), "https://en.wikipedia.org/wiki/Special:Search?search=".to_string(), search_text.to_string())
    } else {
        let q_text = q[1..].to_string();
        ("duckduckgo".to_string(), "https://duckduckgo.com/?q=".to_string(), q_text)
    };

    if query_text.is_empty() {
        return None;
    }

    let encoded_query = url_encode(&query_text);
    Some(WebHistoryItem { query: query_text, engine: engine_name, url: format!("{}{}", search_url, encoded_query) })
}

pub(crate) fn parse_desktop_file(path: &Path) -> Option<AppInfo> {
    let content = fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut no_display = false;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }

        if let Some(pos) = line.find('=') {
            let key = line[..pos].trim();
            let val = line[pos + 1..].trim();

            match key {
                "Name" if name.is_none() => name = Some(val.to_string()),
                "Exec" if exec.is_none() => {
                    let cleaned = val
                        .split_whitespace()
                        .filter(|arg| !arg.starts_with('%'))
                        .collect::<Vec<_>>()
                        .join(" ");
                    exec = Some(cleaned);
                }
                "Icon" if icon.is_none() => icon = Some(val.to_string()),
                "NoDisplay" if val.to_lowercase() == "true" => no_display = true,
                _ => {}
            }
        }
    }

    if no_display {
        return None;
    }

    if let (Some(n), Some(e)) = (name, exec) {
        Some(AppInfo { name: n, exec: e, icon: icon.unwrap_or_default(), count: 0 })
    } else {
        None
    }
}

pub(crate) fn init_db() -> Result<Connection, rusqlite::Error> {
    let db = db_path();
    if let Some(parent) = db.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = Connection::open(&db)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    init_schema(&conn)?;
    Ok(conn)
}

pub(crate) fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS bookmarks (
            url TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS file_history (
            path TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            timestamp INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS web_search_history (
            query TEXT COLLATE NOCASE NOT NULL,
            engine TEXT NOT NULL,
            url TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            PRIMARY KEY(query, engine)
        );
        CREATE TABLE IF NOT EXISTS app_usage (
            app_name TEXT PRIMARY KEY,
            count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_file_history_timestamp ON file_history(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_web_search_history_timestamp ON web_search_history(timestamp DESC);
        "#,
    )?;
    // Legacy migration.
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn migrate_if_needed(conn: &Connection, cache_dir: &Path) {
    // 1. bookmarks
    let bookmarks_path = cache_dir.join("bookmarks.json");
    if bookmarks_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&bookmarks_path)
            && let Ok(list) = serde_json::from_str::<Vec<BookmarkItem>>(&content)
        {
            let now = now_secs();
            let mut success = true;
            for (idx, item) in list.iter().enumerate() {
                let normalized = normalize_url(&item.url);
                if let Err(e) = conn.execute(
                    "INSERT OR IGNORE INTO bookmarks (url, name, created_at) VALUES (?1, ?2, ?3)",
                    params![normalized, item.name, now - idx as i64],
                ) {
                    eprintln!("Error migrating bookmark: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&bookmarks_path, cache_dir.join("bookmarks.json.migrated"));
        }
    }

    // 2. file history
    let file_history_path = cache_dir.join("file_history.json");
    if file_history_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&file_history_path)
            && let Ok(list) = serde_json::from_str::<Vec<FileHistoryItem>>(&content)
        {
            let mut success = true;
            for item in list {
                if let Err(e) = conn.execute(
                    "INSERT OR IGNORE INTO file_history (path, name, timestamp) VALUES (?1, ?2, ?3)",
                    params![item.path, item.name, item.timestamp],
                ) {
                    eprintln!("Error migrating file history: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&file_history_path, cache_dir.join("file_history.json.migrated"));
        }
    }

    // 3. web history
    let web_history_path = cache_dir.join("web_search_history.json");
    if web_history_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&web_history_path)
            && let Ok(list) = serde_json::from_str::<Vec<WebHistoryItem>>(&content)
        {
            let now = now_secs();
            let mut success = true;
            for (idx, item) in list.iter().enumerate() {
                if let Err(e) = conn.execute(
                    "INSERT OR IGNORE INTO web_search_history (query, engine, url, timestamp) VALUES (?1, ?2, ?3, ?4)",
                    params![item.query, item.engine, item.url, now - idx as i64],
                ) {
                    eprintln!("Error migrating web history: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&web_history_path, cache_dir.join("web_search_history.json.migrated"));
        }
    }

    // 4. usage
    let usage_path = cache_dir.join("app_usage.json");
    if usage_path.exists() {
        let mut migrated = false;
        if let Ok(content) = fs::read_to_string(&usage_path)
            && let Ok(map) = serde_json::from_str::<HashMap<String, u32>>(&content)
        {
            let mut success = true;
            for (app_name, count) in map {
                if let Err(e) = conn.execute(
                    "INSERT OR REPLACE INTO app_usage (app_name, count) VALUES (?1, ?2)",
                    params![app_name, count],
                ) {
                    eprintln!("Error migrating app usage: {}", e);
                    success = false;
                }
            }
            if success {
                migrated = true;
            }
        }
        if migrated {
            let _ = fs::rename(&usage_path, cache_dir.join("app_usage.json.migrated"));
        }
    }
}

pub(crate) fn open_db() -> Result<Connection, rusqlite::Error> {
    let conn = init_db()?;
    let cache = cache_dir();
    let _ = fs::create_dir_all(&cache);
    migrate_if_needed(&conn, &cache);
    Ok(conn)
}

fn get_bookmarks(conn: &Connection) -> Vec<BookmarkItem> {
    let Ok(mut stmt) = conn.prepare("SELECT url, name FROM bookmarks ORDER BY created_at DESC") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| Ok(BookmarkItem { url: row.get(0)?, name: row.get(1)? })) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn add_bookmark(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    let normalized_url = normalize_url(url);
    let name = normalized_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or(&normalized_url)
        .to_string();
    conn.execute(
        "INSERT OR REPLACE INTO bookmarks (url, name, created_at) VALUES (?1, ?2, ?3)",
        params![normalized_url, name, now_secs()],
    )?;
    Ok(())
}

fn delete_bookmark(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM bookmarks WHERE url = ?1", params![normalize_url(url)])?;
    Ok(())
}

fn clear_bookmarks(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM bookmarks", [])?;
    Ok(())
}

fn get_file_history(conn: &Connection) -> Vec<FileHistoryItem> {
    let Ok(mut stmt) = conn.prepare("SELECT path, name, timestamp FROM file_history ORDER BY timestamp DESC LIMIT 30") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok(FileHistoryItem { path: row.get(0)?, name: row.get(1)?, timestamp: row.get(2)? })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn add_file_history(conn: &Connection, file_path: &str) -> Result<(), rusqlite::Error> {
    let name = Path::new(file_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file_path)
        .to_string();
    conn.execute(
        "INSERT OR REPLACE INTO file_history (path, name, timestamp) VALUES (?1, ?2, ?3)",
        params![file_path, name, now_secs()],
    )?;
    Ok(())
}

fn clear_file_history(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM file_history", [])?;
    Ok(())
}

fn get_web_history(conn: &Connection) -> Vec<WebHistoryItem> {
    let Ok(mut stmt) = conn.prepare("SELECT query, engine, url FROM web_search_history ORDER BY timestamp DESC LIMIT 20") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok(WebHistoryItem { query: row.get(0)?, engine: row.get(1)?, url: row.get(2)? })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn add_web_history(conn: &Connection, item: &WebHistoryItem) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO web_search_history (query, engine, url, timestamp) VALUES (?1, ?2, ?3, ?4)",
        params![item.query, item.engine, item.url, now_secs()],
    )?;
    Ok(())
}

fn clear_web_history(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM web_search_history", [])?;
    Ok(())
}

fn get_usage_map(conn: &Connection) -> HashMap<String, u32> {
    let Ok(mut stmt) = conn.prepare("SELECT app_name, count FROM app_usage") else {
        return HashMap::new();
    };
    let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))) else {
        return HashMap::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn increment_app_usage(conn: &Connection, app_name: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO app_usage (app_name, count) VALUES (?1, 1) ON CONFLICT(app_name) DO UPDATE SET count = count + 1",
        params![app_name],
    )?;
    Ok(())
}

pub(crate) fn collect_apps(conn: &Connection) -> (Vec<AppInfo>, Vec<AppInfo>) {
    let home = home_dir_str();
    let usage_map = get_usage_map(conn);

    let mut apps: HashMap<String, AppInfo> = HashMap::new();

    let mut paths: Vec<String> = Vec::new();

    // 1. XDG_DATA_HOME / ~/.local/share/applications (highest priority)
    let xdg_data_home = env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{}/.local/share", home));
    paths.push(format!("{}/applications", xdg_data_home));

    // 2. Flatpak user
    paths.push(format!("{}/.local/share/flatpak/exports/share/applications", home));

    // 3. XDG_DATA_DIRS
    if let Ok(xdg_data_dirs) = env::var("XDG_DATA_DIRS") {
        for dir in xdg_data_dirs.split(':') {
            if !dir.is_empty() {
                paths.push(format!("{}/applications", dir));
            }
        }
    } else {
        paths.push("/usr/local/share/applications".to_string());
        paths.push("/usr/share/applications".to_string());
    }

    // 4. Flatpak system
    paths.push("/var/lib/flatpak/exports/share/applications".to_string());

    for dir_path in &paths {
        let path = Path::new(dir_path);
        if !path.exists() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|ext| ext == "desktop") {
                    if let Some(file_name) = p.file_name().and_then(|f| f.to_str()) {
                        if let Some(mut app_info) = parse_desktop_file(&p) {
                            if let Some(&count) = usage_map.get(&app_info.name) {
                                app_info.count = count;
                            }
                            apps.entry(file_name.to_string()).or_insert(app_info);
                        }
                    }
                }
            }
        }
    }

    let mut all_apps: Vec<AppInfo> = apps.into_values().collect();
    all_apps.sort_by_key(|a| a.name.to_lowercase());

    let mut most_used: Vec<AppInfo> = all_apps.iter().filter(|app| app.count > 0).cloned().collect();
    most_used.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    most_used.truncate(5);

    (all_apps, most_used)
}

pub(crate) fn collect_response(conn: &Connection) -> AppsResponse {
    let (all_apps, most_used) = collect_apps(conn);
    AppsResponse {
        most_used,
        all_apps,
        web_history: get_web_history(conn),
        file_history: get_file_history(conn),
        bookmarks: get_bookmarks(conn),
    }
}

fn open_browser(item: &WebHistoryItem) {
    let _ = crate::proc::run(&xdg_open(), &[&item.url]);
    let _ = crate::proc::run(&hyprctl(), &["dispatch", "hl.dsp.focus({workspace=1})"]);
}

// File index/search.

fn index_files() -> String {
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

fn search_files(query: &str) -> String {
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
            if let Some(c2) = child {
                if let Ok(output) = c2.wait_with_output()
                    && output.status.success()
                {
                    fzf_lines = Some(String::from_utf8_lossy(&output.stdout).lines().map(|s| s.to_string()).collect());
                }
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

fn open_file(file_path: &str) -> String {
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

fn render_apps(resp: &AppsResponse) -> String {
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

pub(crate) fn usage() -> String {
    "apps — app launcher backend (desktop apps, bookmarks, history).\n\nUsage:\n  alexie apps [list] [--json] [--search TEXT]\n  alexie apps launch <name>               # record usage\n  alexie apps bookmark add <url> | remove <url> | list [--json] | clear\n  alexie apps history web clear | file clear | file list [--json]\n  alexie apps web search <query...>     # parse !g/!yt/… and open\n  alexie apps web open <query> <engine> <url>\n  alexie apps files index | search <query> | open <path> | history\n  # wabi flags: --launch NAME, --add-bookmark URL, --delete-bookmark URL,\n  # --clear-bookmarks, --clear-history, --clear-file-history, --index-files,\n  # --search-files QUERY, --open-file PATH, --file-history,\n  # --web-search QUERY, --web-search-item Q E U\n".to_string()
}

pub fn handle(args: &[String]) -> String {
    // Wabi flag aliases.
    if let Some(flag) = args.first().map(|s| s.as_str()) {
        if flag.starts_with("--") {
            return handle_wabi_flag(args);
        }
    }

    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "-h" | "--help" | "help" => usage(),
        "" | "list" => {
            let want_json = args.iter().any(|a| a == "--json");
            let search = {
                let mut s = None;
                let mut i = 0;
                while i < args.len() {
                    if args[i] == "--search" && i + 1 < args.len() {
                        s = Some(args[i + 1].clone());
                    }
                    i += 1;
                }
                s
            };
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            let mut resp = collect_response(&conn);
            if let Some(q) = search {
                let ql = q.to_lowercase();
                resp.all_apps.retain(|a| a.name.to_lowercase().contains(&ql) || a.exec.to_lowercase().contains(&ql));
                resp.most_used.retain(|a| a.name.to_lowercase().contains(&ql) || a.exec.to_lowercase().contains(&ql));
            }
            if want_json {
                serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
            } else {
                render_apps(&resp)
            }
        }
        "launch" => {
            if args.len() < 2 {
                return "ERR usage: apps launch <name>".to_string();
            }
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            match increment_app_usage(&conn, &args[1]) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {}", e),
            }
        }
        "bookmark" => {
            let bsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            match bsub {
                "add" => {
                    if args.len() < 3 {
                        return "ERR usage: apps bookmark add <url>".to_string();
                    }
                    match add_bookmark(&conn, &args[2]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
                }
                "remove" | "delete" => {
                    if args.len() < 3 {
                        return "ERR usage: apps bookmark remove <url>".to_string();
                    }
                    match delete_bookmark(&conn, &args[2]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
                }
                "list" => {
                    let want_json = args.iter().any(|a| a == "--json");
                    let items = get_bookmarks(&conn);
                    if want_json {
                        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                    } else if items.is_empty() {
                        "bookmarks: none".to_string()
                    } else {
                        let rows: Vec<Vec<String>> = items.iter().map(|b| vec![b.name.clone(), b.url.clone()]).collect();
                        crate::table::render(&["Name", "URL"], &rows)
                    }
                }
                "clear" => match clear_bookmarks(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                _ => format!("ERR unknown subcommand: bookmark {}", bsub),
            }
        }
        "history" => {
            let hsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            match hsub {
                "web" => {
                    let wsub = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    match wsub {
                        "clear" => match clear_web_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                        "list" => {
                            let want_json = args.iter().any(|a| a == "--json");
                            let items = get_web_history(&conn);
                            if want_json {
                                serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                            } else if items.is_empty() {
                                "web history: none".to_string()
                            } else {
                                let rows: Vec<Vec<String>> = items.iter().map(|h| vec![h.query.clone(), h.engine.clone(), h.url.clone()]).collect();
                                crate::table::render(&["Query", "Engine", "URL"], &rows)
                            }
                        }
                        _ => format!("ERR unknown subcommand: history web {}", wsub),
                    }
                }
                "file" => {
                    let fsub = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    match fsub {
                        "clear" => match clear_file_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                        "list" => {
                            let want_json = args.iter().any(|a| a == "--json");
                            let items = get_file_history(&conn);
                            if want_json {
                                serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                            } else if items.is_empty() {
                                "file history: none".to_string()
                            } else {
                                let rows: Vec<Vec<String>> = items.iter().map(|h| vec![h.path.clone(), h.name.clone()]).collect();
                                crate::table::render(&["Path", "Name"], &rows)
                            }
                        }
                        _ => format!("ERR unknown subcommand: history file {}", fsub),
                    }
                }
                _ => format!("ERR usage: apps history <web|file> <clear|list>"),
            }
        }
        "web" => {
            let wsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match wsub {
                "search" => {
                    if args.len() < 3 {
                        return "ERR usage: apps web search <query...>".to_string();
                    }
                    let query = args[2..].join(" ");
                    let Some(item) = parse_web_search(&query) else {
                        return "ERR web search needs a !-trigger (e.g. !g rust)".to_string();
                    };
                    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
                    if let Err(e) = add_web_history(&conn, &item) {
                        eprintln!("Error saving web search history: {}", e);
                    }
                    open_browser(&item);
                    item.url
                }
                "open" => {
                    if args.len() < 5 {
                        return "ERR usage: apps web open <query> <engine> <url>".to_string();
                    }
                    let item = WebHistoryItem { query: args[2].clone(), engine: args[3].clone(), url: args[4].clone() };
                    if item.query.trim().is_empty() || item.url.trim().is_empty() {
                        return "ERR web open needs query and url".to_string();
                    }
                    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
                    if let Err(e) = add_web_history(&conn, &item) {
                        eprintln!("Error saving web search history: {}", e);
                    }
                    open_browser(&item);
                    item.url
                }
                _ => format!("ERR usage: apps web <search|open> …"),
            }
        }
        "files" => {
            let fsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match fsub {
                "index" => index_files(),
                "search" => {
                    if args.len() < 3 {
                        return "ERR usage: apps files search <query>".to_string();
                    }
                    let query = args[2..].join(" ");
                    search_files(&query)
                }
                "open" => {
                    if args.len() < 3 {
                        return "ERR usage: apps files open <path>".to_string();
                    }
                    open_file(&args[2])
                }
                "history" => {
                    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
                    let items = get_file_history(&conn);
                    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                }
                _ => format!("ERR usage: apps files <index|search|open|history> …"),
            }
        }
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}

fn handle_wabi_flag(args: &[String]) -> String {
    let flag = args[0].as_str();
    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
    match flag {
        "--clear-history" => match clear_web_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
        "--clear-file-history" => match clear_file_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
        "--clear-bookmarks" => match clear_bookmarks(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
        "--add-bookmark" => {
            if args.len() < 2 {
                return "ERR usage: apps --add-bookmark <url>".to_string();
            }
            match add_bookmark(&conn, &args[1]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
        }
        "--delete-bookmark" => {
            if args.len() < 2 {
                return "ERR usage: apps --delete-bookmark <url>".to_string();
            }
            match delete_bookmark(&conn, &args[1]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
        }
        "--launch" => {
            if args.len() < 2 {
                return "ERR usage: apps --launch <name>".to_string();
            }
            match increment_app_usage(&conn, &args[1]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
        }
        "--index-files" => index_files(),
        "--search-files" => {
            if args.len() < 2 {
                return "ERR usage: apps --search-files <query>".to_string();
            }
            search_files(&args[1])
        }
        "--open-file" => {
            if args.len() < 2 {
                return "ERR usage: apps --open-file <path>".to_string();
            }
            open_file(&args[1])
        }
        "--file-history" => serde_json::to_string(&get_file_history(&conn)).unwrap_or_else(|_| "[]".to_string()),
        "--web-search" => {
            if args.len() < 2 {
                return "ERR usage: apps --web-search <query>".to_string();
            }
            let Some(item) = parse_web_search(&args[1]) else {
                return "ERR web search needs a !-trigger".to_string();
            };
            if let Err(e) = add_web_history(&conn, &item) {
                eprintln!("Error saving web search history: {}", e);
            }
            open_browser(&item);
            "OK".to_string()
        }
        "--web-search-item" => {
            if args.len() < 4 {
                return "ERR usage: apps --web-search-item <query> <engine> <url>".to_string();
            }
            let item = WebHistoryItem { query: args[1].clone(), engine: args[2].clone(), url: args[3].clone() };
            if item.query.trim().is_empty() || item.url.trim().is_empty() {
                return "ERR web search item needs query and url".to_string();
            }
            if let Err(e) = add_web_history(&conn, &item) {
                eprintln!("Error saving web search history: {}", e);
            }
            open_browser(&item);
            "OK".to_string()
        }
        _ => format!("ERR unknown flag: {}", flag),
    }
}

/// JSON for D-Bus.
pub(crate) fn list_json() -> String {
    match open_db() {
        Ok(conn) => {
            let resp = collect_response(&conn);
            serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
        }
        Err(_) => serde_json::json!({ "most_used": [], "all_apps": [], "web_history": [], "file_history": [], "bookmarks": [] }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_normalize_url() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("example.com/"), "https://example.com");
        assert_eq!(
            normalize_url("https://Mail.Google.com/Mail/u/0/"),
            "https://mail.google.com/Mail/u/0"
        );
        assert_eq!(normalize_url("http://foo.bar/"), "http://foo.bar");
    }

    #[test]
    fn test_parse_web_search() {
        let item = parse_web_search("!g rust").unwrap();
        assert_eq!(item.engine, "google");
        assert_eq!(item.query, "rust");
        assert_eq!(item.url, "https://www.google.com/search?q=rust");
        let yt = parse_web_search("!yt lofi").unwrap();
        assert_eq!(yt.engine, "youtube");
        assert!(yt.url.contains("search_query=lofi"));
        assert!(parse_web_search("rust").is_none());
        assert!(parse_web_search("!g").is_none());
    }

    #[test]
    fn url_encode_keeps_safe_chars() {
        assert_eq!(url_encode("a b/c"), "a+b%2Fc");
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn parse_desktop_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "alexie-apps-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("kitty.desktop");
        std::fs::write(&p, "[Desktop Entry]\nName=Kitty\nExec=kitty --single-instance %U\nIcon=kitty\n").unwrap();
        let app = parse_desktop_file(&p).unwrap();
        assert_eq!(app.name, "Kitty");
        assert_eq!(app.exec, "kitty --single-instance");
        assert_eq!(app.icon, "kitty");

        // NoDisplay hides
        let p2 = dir.join("hidden.desktop");
        std::fs::write(&p2, "[Desktop Entry]\nName=Hidden\nExec=x\nNoDisplay=true\n").unwrap();
        assert!(parse_desktop_file(&p2).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bookmark_roundtrip_normalizes() {
        let conn = open_mem();
        add_bookmark(&conn, "example.com/docs/").unwrap();
        let items = get_bookmarks(&conn);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://example.com/docs");
        delete_bookmark(&conn, "https://example.com/docs").unwrap();
        assert!(get_bookmarks(&conn).is_empty());
    }

    #[test]
    fn usage_increments_and_collects() {
        let conn = open_mem();
        increment_app_usage(&conn, "Kitty").unwrap();
        increment_app_usage(&conn, "Kitty").unwrap();
        let map = get_usage_map(&conn);
        assert_eq!(map.get("Kitty"), Some(&2));
    }

    #[test]
    fn web_and_file_history_roundtrips() {
        let conn = open_mem();
        let item = WebHistoryItem { query: "rust".into(), engine: "google".into(), url: "https://www.google.com/search?q=rust".into() };
        add_web_history(&conn, &item).unwrap();
        assert_eq!(get_web_history(&conn).len(), 1);
        add_file_history(&conn, "/home/u/doc.txt").unwrap();
        assert_eq!(get_file_history(&conn)[0].name, "doc.txt");
        clear_web_history(&conn).unwrap();
        clear_file_history(&conn).unwrap();
        assert!(get_web_history(&conn).is_empty());
        assert!(get_file_history(&conn).is_empty());
    }

    #[test]
    fn collect_response_shapes_match_wabi() {
        let conn = open_mem();
        let resp = collect_response(&conn);
        let v = serde_json::to_value(&resp).unwrap();
        for k in ["most_used", "all_apps", "web_history", "file_history", "bookmarks"] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
    }

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }

    #[test]
    fn usage_lists_commands() {
        let u = usage();
        assert!(u.contains("apps"));
        assert!(u.contains("--launch"));
    }
}
