//! Audio structs + current-player file state.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
pub(crate) fn current_player_path() -> PathBuf {
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(rt).join("quickshell_current_media_player")
}

pub(crate) fn load_current_player() -> Option<String> {
    fs::read_to_string(current_player_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[allow(dead_code)]
pub(crate) fn save_current_player(name: &str) {
    let _ = fs::write(current_player_path(), name);
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SinkInfo {
    pub index: serde_json::Value,
    pub name: String,
    pub description: String,
    pub volume: i64,
    pub muted: bool,
    pub is_bluetooth: bool,
    pub sample_rate: String,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SourceInfo {
    pub index: serde_json::Value,
    pub name: String,
    pub description: String,
    pub volume: i64,
    pub muted: bool,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub index: serde_json::Value,
    pub name: String,
    pub volume: i64,
    pub muted: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Diagnostics {
    pub pipewire_version: String,
    pub sample_rate: String,
    pub output_desc: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaInfo {
    pub player: String,
    pub title: String,
    pub artist: String,
    pub art_url: String,
    pub status: String,
    pub position: f64,
    pub length: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaSource {
    pub name: String,
    pub status: String,
    pub title: String,
    pub artist: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AudioResult {
    pub default_sink: Option<SinkInfo>,
    pub default_source: Option<SourceInfo>,
    pub sinks: Vec<SinkInfo>,
    pub sources: Vec<SourceInfo>,
    pub apps: Vec<AppInfo>,
    pub diagnostics: Diagnostics,
    pub media: Option<MediaInfo>,
    pub media_sources: Vec<MediaSource>,
    pub current_media_source: Option<String>,
}
