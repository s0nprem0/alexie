//! URL encode/normalize, web-search triggers, desktop-file parsing.

use super::types::{AppInfo, WebHistoryItem};
use std::fs;
use std::path::Path;

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
        (
            "youtube".to_string(),
            "https://www.youtube.com/results?search_query=".to_string(),
            search_text.to_string(),
        )
    } else if trigger == "!g" || trigger == "!google" {
        (
            "google".to_string(),
            "https://www.google.com/search?q=".to_string(),
            search_text.to_string(),
        )
    } else if trigger == "!gh" || trigger == "!github" {
        (
            "github".to_string(),
            "https://github.com/search?q=".to_string(),
            search_text.to_string(),
        )
    } else if trigger == "!w" || trigger == "!wiki" || trigger == "!wikipedia" {
        (
            "wikipedia".to_string(),
            "https://en.wikipedia.org/wiki/Special:Search?search=".to_string(),
            search_text.to_string(),
        )
    } else {
        let q_text = q[1..].to_string();
        (
            "duckduckgo".to_string(),
            "https://duckduckgo.com/?q=".to_string(),
            q_text,
        )
    };

    if query_text.is_empty() {
        return None;
    }

    let encoded_query = url_encode(&query_text);
    Some(WebHistoryItem {
        query: query_text,
        engine: engine_name,
        url: format!("{}{}", search_url, encoded_query),
    })
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
        Some(AppInfo {
            name: n,
            exec: e,
            icon: icon.unwrap_or_default(),
            count: 0,
        })
    } else {
        None
    }
}
