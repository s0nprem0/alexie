//! Serializable structs.

use serde::{Deserialize, Serialize};

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
