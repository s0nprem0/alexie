//! Desktop-app collection + usage aggregation.

use rusqlite::Connection;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use super::db::{get_bookmarks, get_file_history, get_usage_map, get_web_history};
use super::types::{AppInfo, AppsResponse};
use super::url::parse_desktop_file;
use super::paths::home_dir_str;

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
                if p.extension().is_some_and(|ext| ext == "desktop")
                    && let Some(file_name) = p.file_name().and_then(|f| f.to_str())
                        && let Some(mut app_info) = parse_desktop_file(&p) {
                            if let Some(&count) = usage_map.get(&app_info.name) {
                                app_info.count = count;
                            }
                            apps.entry(file_name.to_string()).or_insert(app_info);
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
