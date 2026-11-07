//! Shot asset DB: screenshots/recordings, colors, OCR.
//!
//! Split from a single 1221-line `db.rs`:
//! - `types` — Asset/AssetType/ProbeResult/MediaStatus
//! - `paths` — env helpers, db/thumbnail/screenshot dirs, clock
//! - `open` — Connection open, schema, legacy migration
//! - `assets` — add/list/remove assets, probe, thumbnails
//! - `colors` — picked colors + OCR history
//! - `status` — recording state, fps, collect_status, indexing
//!
//! `handle()` below is just argument parsing/dispatch + rendering.

pub(crate) mod types;
pub(crate) mod paths;
pub(crate) mod open;
pub(crate) mod assets;
pub(crate) mod colors;
pub(crate) mod status;

#[allow(unused_imports)] // re-exported for public API compat
pub use types::{Asset, AssetType, MediaStatus, OcrItem, PickedColor, ProbeResult, SCHEMA_VERSION, THUMB_SIZE};

use rusqlite::Connection;
use paths::{db_path, thumbnails_dir};
use open::open;
use assets::{add_asset, check_deleted, clear_all, clear_assets, list_assets, remove_asset};
use colors::{add_color, add_ocr, clear_colors, clear_ocr, list_colors, list_ocr, remove_color};
use status::{collect_status, is_recording};
pub(crate) use status::{index_recording, index_screenshot};
use paths::hyprpicker;

#[cfg(test)]
use open::init_schema;
#[cfg(test)]
use assets::{parse_probe_json, thumbnail_path_for};

pub(crate) fn usage() -> String {
    "shot db — screenshot/recording asset index (wabi media_db).\n\nUsage:\n  alexie shot db [list] [--json] [--limit N] [--search TEXT]\n  alexie shot db status [--json]\n  alexie shot db add <screenshot|recording> <path>\n  alexie shot db remove <id>\n  alexie shot db clear [assets|ocr|colors|all]\n  alexie shot db check               # mark deleted files\n  alexie shot db path|thumbs         # print DB / thumbnails dir\n  alexie shot db color add <hex> | list [--json] | remove <id> | clear\n  alexie shot db ocr add <detail> | list [--json] | clear\n  # wabi aliases: add-asset, remove-asset, clear-assets, clear-ocr,\n  # clear-all, clear-colors, add-color, remove-color, pick-color,\n  # recording-status, add ocr <detail>\n".to_string()
}

fn open_or_err() -> Result<Connection, String> {
    open().map_err(|e| format!("ERR db: {}", e))
}

fn render_assets(assets: &[Asset]) -> String {
    if assets.is_empty() {
        return "db: no assets".to_string();
    }
    let rows: Vec<Vec<String>> = assets
        .iter()
        .map(|a| {
            let size = a.file_size.map(|s| {
                if s >= 1024 * 1024 {
                    format!("{:.1}MiB", s as f64 / 1024.0 / 1024.0)
                } else if s >= 1024 {
                    format!("{:.1}KiB", s as f64 / 1024.0)
                } else {
                    format!("{}B", s)
                }
            }).unwrap_or_else(|| "-".to_string());
            let dims = match (a.width, a.height, a.duration_ms) {
                (Some(w), Some(h), _) => format!("{}x{}", w, h),
                (_, _, Some(ms)) => format!("{}ms", ms),
                _ => "-".to_string(),
            };
            vec![
                a.id.to_string(),
                a.asset_type.clone(),
                a.source_path.clone(),
                size,
                dims,
            ]
        })
        .collect();
    crate::table::render(&["ID", "Type", "Path", "Size", "WxH"], &rows)
}

fn render_colors(colors: &[PickedColor]) -> String {
    if colors.is_empty() {
        return "db: no colors".to_string();
    }
    let rows: Vec<Vec<String>> = colors
        .iter()
        .map(|c| vec![c.id.to_string(), c.hex.clone()])
        .collect();
    crate::table::render(&["ID", "Hex"], &rows)
}

fn render_ocr(items: &[OcrItem]) -> String {
    if items.is_empty() {
        return "db: no ocr history".to_string();
    }
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|o| vec![o.id.to_string(), o.detail.clone()])
        .collect();
    crate::table::render(&["ID", "Text"], &rows)
}

fn parse_limit(args: &[String], default: i64) -> i64 {
    let mut limit = default;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--limit" && i + 1 < args.len()
            && let Ok(v) = args[i + 1].parse::<i64>() {
                limit = v.clamp(1, 500);
            }
        i += 1;
    }
    limit
}

fn parse_search(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--search" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

fn render_status_text(s: &MediaStatus) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "recording: {}  fps: {}  shots: {}  recordings: {}\n",
        if s.is_recording { "on" } else { "off" },
        s.monitor_fps,
        s.screenshot_dir,
        s.recording_dir
    ));
    out.push_str(&render_assets(&s.assets));
    out.push_str("\n\n");
    out.push_str(&render_colors(&s.colors));
    out.push_str("\n\n");
    out.push_str(&render_ocr(&s.history));
    out.trim_end().to_string()
}

pub fn handle(args: &[String]) -> String {
    // Wabi aliases.
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");

    match sub {
        "-h" | "--help" | "help" => return usage(),
        // Wabi aliases.
        "add-asset" => {
            if args.len() < 3 {
                return "ERR usage: shot db add-asset <type> <path>".to_string();
            }
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match add_asset(&conn, &args[1], &args[2]) {
                Ok(id) => id.to_string(),
                Err(e) => format!("ERR {}", e),
            };
        }
        "remove-asset" => {
            if args.len() < 2 {
                return "ERR usage: shot db remove-asset <id>".to_string();
            }
            let Ok(id) = args[1].parse::<i64>() else { return "ERR invalid asset id".to_string() };
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match remove_asset(&conn, id) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {}", e),
            };
        }
        "clear-assets" => {
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match clear_assets(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) };
        }
        "clear-ocr" => {
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match clear_ocr(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) };
        }
        "clear-all" => {
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match clear_all(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) };
        }
        "clear-colors" => {
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match clear_colors(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) };
        }
        "add-color" => {
            if args.len() < 2 {
                return "ERR usage: shot db add-color <hex>".to_string();
            }
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match add_color(&conn, &args[1]) { Ok(id) => id.to_string(), Err(e) => format!("ERR {}", e) };
        }
        "remove-color" => {
            if args.len() < 2 {
                return "ERR usage: shot db remove-color <id>".to_string();
            }
            let Ok(id) = args[1].parse::<i64>() else { return "ERR invalid color id".to_string() };
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match remove_color(&conn, id) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) };
        }
        "pick-color" => {
            let (code, out, err) = crate::proc::run(&hyprpicker(), &["-a", "-n"]);
            if code != 0 {
                return format!("ERR hyprpicker rc={} {}", code, err.trim());
            }
            let hex = out.trim().to_string();
            if !hex.starts_with('#') {
                return format!("ERR no hex from hyprpicker: '{}'", hex);
            }
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match add_color(&conn, &hex) { Ok(id) => id.to_string(), Err(e) => format!("ERR {}", e) };
        }
        "recording-status" => {
            return serde_json::json!({ "is_recording": is_recording() }).to_string();
        }
        // `add ocr <detail>`
        "add" => {
            if args.get(1).map(|s| s.as_str()) == Some("ocr") {
                if args.len() < 3 {
                    return "ERR usage: shot db add ocr <detail>".to_string();
                }
                let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                let detail = args[2..].join(" ");
                return match add_ocr(&conn, &detail) { Ok(id) => id.to_string(), Err(e) => format!("ERR {}", e) };
            }
            if args.len() < 3 {
                return "ERR usage: shot db add <screenshot|recording> <path> | shot db add ocr <detail>".to_string();
            }
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            return match add_asset(&conn, &args[1], &args[2]) {
                Ok(id) => id.to_string(),
                Err(e) => format!("ERR {}", e),
            };
        }
        _ => {}
    }

    match sub {
        "" | "list" => {
            let want_json = args.iter().any(|a| a == "--json");
            let limit = parse_limit(args, 50);
            let search = parse_search(args);
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            let _ = check_deleted(&conn);
            let assets = list_assets(&conn, search.as_deref(), limit).unwrap_or_default();
            if want_json {
                serde_json::to_string(&assets).unwrap_or_else(|_| "[]".to_string())
            } else {
                render_assets(&assets)
            }
        }
        "status" => {
            let want_json = args.iter().any(|a| a == "--json");
            let status = collect_status();
            if want_json {
                serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string())
            } else {
                render_status_text(&status)
            }
        }
        "remove" => {
            if args.len() < 2 {
                return "ERR usage: shot db remove <id>".to_string();
            }
            let Ok(id) = args[1].parse::<i64>() else { return "ERR invalid asset id".to_string() };
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            match remove_asset(&conn, id) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {}", e),
            }
        }
        "clear" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("assets");
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            match what {
                "assets" => match clear_assets(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                "ocr" => match clear_ocr(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                "colors" => match clear_colors(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                "all" => match clear_all(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                _ => format!("ERR usage: shot db clear [assets|ocr|colors|all], got: {}", what),
            }
        }
        "check" => {
            let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
            match check_deleted(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
        }
        "path" => db_path().to_string_lossy().to_string(),
        "thumbs" => thumbnails_dir().to_string_lossy().to_string(),
        "color" => {
            let csub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match csub {
                "add" => {
                    if args.len() < 3 {
                        return "ERR usage: shot db color add <hex>".to_string();
                    }
                    let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                    match add_color(&conn, &args[2]) { Ok(id) => id.to_string(), Err(e) => format!("ERR {}", e) }
                }
                "list" => {
                    let want_json = args.iter().any(|a| a == "--json");
                    let limit = parse_limit(args, 24);
                    let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                    let colors = list_colors(&conn, limit).unwrap_or_default();
                    if want_json {
                        serde_json::to_string(&colors).unwrap_or_else(|_| "[]".to_string())
                    } else {
                        render_colors(&colors)
                    }
                }
                "remove" => {
                    if args.len() < 3 {
                        return "ERR usage: shot db color remove <id>".to_string();
                    }
                    let Ok(id) = args[2].parse::<i64>() else { return "ERR invalid color id".to_string() };
                    let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                    match remove_color(&conn, id) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
                }
                "clear" => {
                    let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                    match clear_colors(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
                }
                _ => format!("ERR unknown subcommand: color {}", csub),
            }
        }
        "ocr" => {
            let osub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match osub {
                "add" => {
                    if args.len() < 3 {
                        return "ERR usage: shot db ocr add <detail>".to_string();
                    }
                    let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                    let detail = args[2..].join(" ");
                    match add_ocr(&conn, &detail) { Ok(id) => id.to_string(), Err(e) => format!("ERR {}", e) }
                }
                "list" => {
                    let want_json = args.iter().any(|a| a == "--json");
                    let limit = parse_limit(args, 50);
                    let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                    let items = list_ocr(&conn, limit).unwrap_or_default();
                    if want_json {
                        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                    } else {
                        render_ocr(&items)
                    }
                }
                "clear" => {
                    let conn = match open_or_err() { Ok(c) => c, Err(e) => return e };
                    match clear_ocr(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
                }
                _ => format!("ERR unknown subcommand: ocr {}", osub),
            }
        }
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.pragma_update(None, "temp_store", "MEMORY").unwrap();
        conn
    }

    #[test]
    fn asset_type_parse_roundtrip() {
        assert_eq!(AssetType::parse("screenshot"), Some(AssetType::Screenshot));
        assert_eq!(AssetType::parse("recording"), Some(AssetType::Recording));
        assert_eq!(AssetType::parse("unknown"), None);
        assert_eq!(AssetType::parse(""), None);
        assert_eq!(AssetType::Screenshot.as_str(), "screenshot");
        assert_eq!(AssetType::Recording.as_str(), "recording");
    }

    #[test]
    fn schema_creates_all_tables_and_indexes() {
        let conn = open_mem();
        init_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO assets (asset_type, source_path, thumbnail_path, created_at) VALUES ('screenshot', '/tmp/test.png', '', 1000)",
            [],
        )
        .unwrap();

        conn.execute("INSERT INTO colors (hex, picked_at) VALUES ('#ff0000', 2000)", []).unwrap();

        conn.execute("INSERT INTO ocr_history (detail, created_at) VALUES ('test ocr', 3000)", []).unwrap();

        let asset_count: i64 = conn.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0)).unwrap();
        let color_count: i64 = conn.query_row("SELECT COUNT(*) FROM colors", [], |r| r.get(0)).unwrap();
        let ocr_count: i64 = conn.query_row("SELECT COUNT(*) FROM ocr_history", [], |r| r.get(0)).unwrap();

        assert_eq!(asset_count, 1);
        assert_eq!(color_count, 1);
        assert_eq!(ocr_count, 1);

        let version: i32 = conn.query_row("SELECT version FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn schema_sets_version_correctly() {
        let conn = open_mem();
        init_schema(&conn).unwrap();
        let version: i32 = conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn add_color_validates_hex_format() {
        let conn = open_mem();
        init_schema(&conn).unwrap();

        assert!(add_color(&conn, "#ff0000").is_ok());
        assert!(add_color(&conn, "#FF00FF").is_ok());
        assert!(add_color(&conn, "#12345678").is_ok());
        assert!(add_color(&conn, "ff0000").is_err());
        assert!(add_color(&conn, "#12345").is_err());
        assert!(add_color(&conn, "").is_err());
    }

    #[test]
    fn add_color_handles_whitespace() {
        let conn = open_mem();
        init_schema(&conn).unwrap();

        let id = add_color(&conn, "  #ff0000  ").unwrap();
        let color = list_colors(&conn, 1).unwrap();
        assert_eq!(color[0].hex, "#ff0000");
        assert_eq!(color[0].id, id);
    }

    #[test]
    fn remove_color_errors_on_nonexistent() {
        let conn = open_mem();
        init_schema(&conn).unwrap();

        assert!(remove_color(&conn, 999).is_err());
        assert!(remove_color(&conn, 0).is_err());
    }

    #[test]
    fn list_colors_returns_empty_for_new_db() {
        let conn = open_mem();
        init_schema(&conn).unwrap();
        let colors = list_colors(&conn, 10).unwrap();
        assert!(colors.is_empty());
    }

    #[test]
    fn add_and_list_ocr_roundtrip() {
        let conn = open_mem();
        init_schema(&conn).unwrap();

        let id = add_ocr(&conn, "Hello from image").unwrap();
        let items = list_ocr(&conn, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, id);
        assert_eq!(items[0].detail, "Hello from image");
        assert_eq!(items[0].item_type, "ocr");
    }

    #[test]
    fn check_deleted_updates_deleted_at() {
        let conn = open_mem();
        init_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO assets (asset_type, source_path, thumbnail_path, created_at) VALUES ('screenshot', '/tmp/certainly-not-a-real-file.png', '', 1000)",
            [],
        )
        .unwrap();

        check_deleted(&conn).unwrap();

        let deleted_at: Option<i64> = conn
            .query_row(
                "SELECT deleted_at FROM assets WHERE source_path = '/tmp/certainly-not-a-real-file.png'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());
    }

    #[test]
    fn thumbnail_path_for_uses_correct_path() {
        let path = thumbnail_path_for(42);
        assert!(path.to_string_lossy().contains("42"));
        assert!(path.to_string_lossy().ends_with(".png"));
        assert!(path.to_string_lossy().contains("thumbnails"));
    }

    #[test]
    fn database_path_has_correct_extension() {
        let path = db_path();
        assert!(path.to_string_lossy().ends_with("assets.db"));
    }

    #[test]
    fn probe_json_parses_streams() {
        let json = r#"{"streams":[{"codec_type":"audio"},{"codec_type":"video","width":1920,"height":1080,"duration":"1.5"}]}"#;
        let p = parse_probe_json(json);
        assert_eq!(p.width, Some(1920));
        assert_eq!(p.height, Some(1080));
        assert_eq!(p.duration_ms, Some(1500));
    }

    #[test]
    fn list_assets_filters_missing_files() {
        let conn = open_mem();
        init_schema(&conn).unwrap();
        // Insert a row whose source does not exist: list_assets must
        // mark it deleted and skip it (zero stale thumbnails).
        conn.execute(
            "INSERT INTO assets (asset_type, source_path, thumbnail_path, created_at) VALUES ('screenshot', '/tmp/certainly-not-a-real-asset.png', '', 1000)",
            [],
        )
        .unwrap();
        let assets = list_assets(&conn, None, 10).unwrap();
        assert!(assets.is_empty());
    }

    #[test]
    fn usage_lists_commands() {
        let u = usage();
        assert!(u.contains("shot db"));
        assert!(u.contains("add-asset"));
    }

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }
}
