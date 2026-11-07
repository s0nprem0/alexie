//! Picked colors + OCR history.

use rusqlite::{Connection, params};
use super::paths::now_millis;
use super::types::{OcrItem, PickedColor};

pub fn add_color(conn: &Connection, hex: &str) -> Result<i64, String> {
    let hex = hex.trim().to_lowercase();
    if !hex.starts_with('#') || (hex.len() != 7 && hex.len() != 9) {
        return Err(format!("invalid hex color: {hex}"));
    }
    let now = now_millis();
    conn.execute(
        "INSERT INTO colors (hex, picked_at) VALUES (?1, ?2)",
        params![hex, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn remove_color(conn: &Connection, id: i64) -> Result<(), String> {
    let rows = conn.execute("DELETE FROM colors WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    if rows == 0 {
        return Err(format!("color id {id} not found"));
    }
    Ok(())
}

pub fn clear_colors(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM colors", []).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_colors(conn: &Connection, limit: i64) -> Result<Vec<PickedColor>, String> {
    let mut stmt = conn
        .prepare("SELECT id, hex, picked_at FROM colors ORDER BY picked_at DESC LIMIT ?1")
        .map_err(|e| e.to_string())?;
    let iter = stmt
        .query_map(params![limit], |r| {
            Ok(PickedColor { id: r.get(0)?, hex: r.get(1)?, picked_at: r.get(2)? })
        })
        .map_err(|e| e.to_string())?;
    Ok(iter.flatten().collect())
}

pub fn add_ocr(conn: &Connection, detail: &str) -> Result<i64, String> {
    let now = now_millis();
    conn.execute(
        "INSERT INTO ocr_history (detail, created_at) VALUES (?1, ?2)",
        params![detail, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn list_ocr(conn: &Connection, limit: i64) -> Result<Vec<OcrItem>, String> {
    let mut stmt = conn
        .prepare("SELECT id, detail, created_at FROM ocr_history ORDER BY created_at DESC LIMIT ?1")
        .map_err(|e| e.to_string())?;
    let iter = stmt
        .query_map(params![limit], |r| {
            Ok(OcrItem { id: r.get(0)?, item_type: "ocr".to_string(), detail: r.get(1)?, created_at: r.get(2)? })
        })
        .map_err(|e| e.to_string())?;
    Ok(iter.flatten().collect())
}

pub fn clear_ocr(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM ocr_history", []).map_err(|e| e.to_string())?;
    Ok(())
}
