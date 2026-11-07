//! Assets: add/list/remove, ffprobe parsing, thumbnails.

use rusqlite::{Connection, OptionalExtension, params};
use std::os::unix::fs as unixfs;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use super::colors::clear_ocr;
use super::paths::{ffmpeg, ffprobe, now_millis, thumbnails_dir};
use super::types::{Asset, AssetType, ProbeResult, THUMB_SIZE};

pub fn thumbnail_path_for(id: i64) -> PathBuf {
    thumbnails_dir().join(format!("{id}.png"))
}

pub(crate) fn thumbnail_path_for_in(id: i64, thumb_dir: &Path) -> PathBuf {
    thumb_dir.join(format!("{id}.png"))
}

fn probe(source: &Path) -> ProbeResult {
    let (code, out, _) = crate::proc::run(
        &ffprobe(),
        &["-v", "quiet", "-print_format", "json", "-show_streams", &source.to_string_lossy()],
    );
    if code != 0 {
        return ProbeResult::default();
    }
    parse_probe_json(&out)
}

pub fn parse_probe_json(json: &str) -> ProbeResult {
    let mut result = ProbeResult::default();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return result;
    };
    let Some(streams) = value.get("streams").and_then(|v| v.as_array()) else {
        return result;
    };
    for stream in streams {
        if stream.get("codec_type").and_then(|v| v.as_str()) != Some("video") {
            continue;
        }
        result.width = stream.get("width").and_then(|v| v.as_i64()).map(|v| v as i32);
        result.height = stream.get("height").and_then(|v| v.as_i64()).map(|v| v as i32);
        if let Some(dur) = stream.get("duration").and_then(|v| v.as_str())
            && let Ok(secs) = dur.parse::<f64>()
        {
            result.duration_ms = Some((secs * 1000.0) as i32);
        }
        break;
    }
    result
}

pub fn add_asset(conn: &Connection, asset_type_str: &str, source: &str) -> Result<i64, String> {
    add_asset_with_thumbs(conn, asset_type_str, source, &thumbnails_dir())
}

pub(crate) fn add_asset_with_thumbs(
    conn: &Connection,
    asset_type_str: &str,
    source: &str,
    thumb_dir: &Path,
) -> Result<i64, String> {
    let asset_type = AssetType::parse(asset_type_str)
        .ok_or_else(|| format!("invalid asset type: {asset_type_str}"))?;
    let source_path = PathBuf::from(source);

    let file_size = fs::metadata(&source_path).ok().map(|m| m.len() as i64);
    let probe_result = if source_path.exists() {
        probe(&source_path)
    } else {
        ProbeResult::default()
    };

    let existing: Option<(i64, Option<i64>)> = conn
        .query_row(
            "SELECT id, deleted_at FROM assets WHERE source_path = ?1",
            params![source],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if let Some((id, _)) = existing {
        conn.execute(
            "UPDATE assets SET file_size = COALESCE(?1, file_size), width = COALESCE(?2, width), height = COALESCE(?3, height), duration_ms = COALESCE(?4, duration_ms), deleted_at = NULL WHERE id = ?5",
            params![file_size, probe_result.width, probe_result.height, probe_result.duration_ms, id],
        )
        .map_err(|e| e.to_string())?;
        ensure_thumbnail_in(&source_path, id, asset_type, thumb_dir);
        return Ok(id);
    }

    let created_at = fs::metadata(&source_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(now_millis);

    conn.execute(
        "INSERT INTO assets (asset_type, source_path, thumbnail_path, created_at, file_size, width, height, duration_ms) VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7)",
        params![
            asset_type.as_str(),
            source,
            created_at,
            file_size,
            probe_result.width,
            probe_result.height,
            probe_result.duration_ms,
        ],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();

    let thumb = thumbnail_path_for_in(id, thumb_dir);
    conn.execute(
        "UPDATE assets SET thumbnail_path = ?1 WHERE id = ?2",
        params![thumb.to_string_lossy().to_string(), id],
    )
    .map_err(|e| e.to_string())?;
    ensure_thumbnail_in(&source_path, id, asset_type, thumb_dir);
    Ok(id)
}

fn ensure_thumbnail_in(source: &Path, id: i64, asset_type: AssetType, thumb_dir: &Path) {
    let thumb = thumbnail_path_for_in(id, thumb_dir);
    if let Some(parent) = thumb.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match asset_type {
        AssetType::Screenshot => {
            if thumb.exists() {
                return;
            }
            if source.exists() {
                let _ = fs::remove_file(&thumb);
                let _ = unixfs::symlink(source, &thumb);
            }
        }
        AssetType::Recording => {
            if thumb.exists() {
                return;
            }
            if !source.exists() {
                return;
            }
            let scale = format!("scale={THUMB_SIZE}:-2");
            let _ = crate::proc::run(
                &ffmpeg(),
                &[
                    "-y", "-hide_banner", "-loglevel", "error", "-i",
                    &source.to_string_lossy(),
                    "-ss", "00:00:00.500", "-vframes", "1", "-vf", &scale,
                    &thumb.to_string_lossy(),
                ],
            );
        }
    }
}

pub fn remove_asset(conn: &Connection, asset_id: i64) -> Result<(), String> {
    let thumb: Option<String> = conn
        .query_row(
            "SELECT thumbnail_path FROM assets WHERE id = ?1",
            params![asset_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(t) = thumb {
        let p = PathBuf::from(&t);
        if p.exists() {
            let _ = fs::remove_file(p);
        }
    }
    conn.execute("DELETE FROM assets WHERE id = ?1", params![asset_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn clear_assets(conn: &Connection) -> Result<(), String> {
    let thumbs: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT thumbnail_path FROM assets")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };
    for t in thumbs {
        let p = PathBuf::from(t);
        if p.exists() {
            let _ = fs::remove_file(p);
        }
    }
    conn.execute("DELETE FROM assets", []).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn clear_all(conn: &Connection) -> Result<(), String> {
    clear_assets(conn)?;
    clear_ocr(conn)?;
    Ok(())
}

pub fn check_deleted(conn: &Connection) -> Result<(), String> {
    let now = now_millis();
    let limit_24h = now - 24 * 3600 * 1000;

    let _ = conn.execute(
        "DELETE FROM assets WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
        params![limit_24h],
    );

    let rows: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, source_path FROM assets WHERE deleted_at IS NULL")
            .map_err(|e| e.to_string())?;
        let iter = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        iter.filter_map(|r| r.ok()).collect()
    };

    for (id, source) in rows {
        if !Path::new(&source).exists() {
            let _ = conn.execute(
                "UPDATE assets SET deleted_at = ?1 WHERE id = ?2",
                params![now, id],
            );
            let thumb = thumbnail_path_for(id);
            let _ = fs::remove_file(&thumb);
        }
    }

    Ok(())
}

pub fn list_assets(conn: &Connection, search: Option<&str>, limit: i64) -> Result<Vec<Asset>, String> {
    let mut sql = String::from(
        "SELECT a.id, a.asset_type, a.source_path, a.thumbnail_path, a.created_at, a.deleted_at, a.file_size, a.width, a.height, a.duration_ms \
         FROM assets a",
    );
    let mut conditions: Vec<String> = Vec::new();
    conditions.push("a.deleted_at IS NULL".to_string());
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(s) = search.filter(|s| !s.is_empty()) {
        let idx = binds.len() + 1;
        conditions.push(format!("a.source_path LIKE ?{idx} ESCAPE '\\'"));
        let pattern = format!(
            "%{}%",
            s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        binds.push(Box::new(pattern));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY a.created_at DESC LIMIT ?");
    let limit_idx = binds.len() + 1;
    sql.push_str(&limit_idx.to_string());
    binds.push(Box::new(limit));

    let mut params_refs: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(binds.len());
    for b in &binds {
        params_refs.push(b.as_ref());
    }

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let asset_iter = stmt
        .query_map(params_refs.as_slice(), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<i32>>(7)?,
                r.get::<_, Option<i32>>(8)?,
                r.get::<_, Option<i32>>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut assets: Vec<Asset> = Vec::new();
    for row in asset_iter {
        let (id, asset_type, source_path, thumbnail_path, created_at, deleted_at, file_size, width, height, duration_ms) =
            row.map_err(|e| e.to_string())?;

        if !Path::new(&source_path).exists() {
            let _ = conn.execute(
                "UPDATE assets SET deleted_at = ?1 WHERE id = ?2",
                params![now_millis(), id],
            );
            let thumb = thumbnail_path_for(id);
            let _ = fs::remove_file(&thumb);
            continue;
        }

        assets.push(Asset {
            id,
            asset_type,
            source_path,
            thumbnail_path,
            created_at,
            deleted: deleted_at.is_some(),
            file_size,
            width,
            height,
            duration_ms,
        });
    }
    Ok(assets)
}
