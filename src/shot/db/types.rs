//! Types, constants, and serializable structs.

use serde::Serialize;

pub const SCHEMA_VERSION: i32 = 2;
pub const THUMB_SIZE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetType {
    Screenshot,
    Recording,
}

impl AssetType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "screenshot" => Some(Self::Screenshot),
            "recording" => Some(Self::Recording),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Screenshot => "screenshot",
            Self::Recording => "recording",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProbeResult {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Asset {
    pub id: i64,
    #[serde(rename = "type")]
    pub asset_type: String,
    pub source_path: String,
    pub thumbnail_path: String,
    pub created_at: i64,
    pub deleted: bool,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct OcrItem {
    pub id: i64,
    #[serde(rename = "type")]
    pub item_type: String,
    pub detail: String,
    pub created_at: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct PickedColor {
    pub id: i64,
    pub hex: String,
    pub picked_at: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct MediaStatus {
    pub is_recording: bool,
    pub screenshot_dir: String,
    pub recording_dir: String,
    pub assets: Vec<Asset>,
    pub history: Vec<OcrItem>,
    pub colors: Vec<PickedColor>,
    pub monitor_fps: u32,
}
