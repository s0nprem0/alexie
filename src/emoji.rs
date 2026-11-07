//! Emoji picker: pick one, copy it to the clipboard, and type it.

use std::env;
use std::fs;
use std::path::PathBuf;

fn emoji_test_path() -> Option<PathBuf> {
    if let Ok(v) = env::var("EMOJI_TEST")
        && !v.is_empty()
    {
        return Some(PathBuf::from(v));
    }
    let cfg = crate::config::get_str(&["emoji", "test_path"], "");
    if !cfg.is_empty() {
        let p = PathBuf::from(&cfg);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut cands: Vec<PathBuf> = vec![
        PathBuf::from("/run/current-system/sw/share/unicode/emoji/emoji-test.txt"),
        PathBuf::from("/usr/share/unicode/emoji/emoji-test.txt"),
        PathBuf::from("/usr/share/unicode/emoji-test.txt"),
    ];
    if let Ok(home) = env::var("HOME") {
        cands.push(PathBuf::from(&home).join(".nix-profile/share/unicode/emoji/emoji-test.txt"));
    }
    for p in &cands {
        if p.is_file() {
            return Some(p.clone());
        }
    }
    // Last resort: scan the Nix store for an installed unicode-emoji.
    if let Ok(entries) = fs::read_dir("/nix/store") {
        for entry in entries.flatten() {
            let p = entry.path().join("share/unicode/emoji/emoji-test.txt");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn wtype() -> String {
    if let Ok(v) = env::var("WTYPE")
        && !v.is_empty()
    {
        return v;
    }
    let cfg = crate::config::get_str(&["emoji", "wtype"], "");
    if !cfg.is_empty() {
        return cfg;
    }
    "wtype".to_string()
}

fn wl_copy() -> String {
    if let Ok(v) = env::var("WL_COPY")
        && !v.is_empty()
    {
        return v;
    }
    let cfg = crate::config::get_str(&["emoji", "wl_copy"], "");
    if !cfg.is_empty() {
        return cfg;
    }
    crate::proc::wl_copy()
}

/// Parse emoji-test.txt `; fully-qualified` lines → `emoji name\temoji` entries.
pub(crate) fn parse_emoji_test(content: &str) -> String {
    let mut out = String::new();
    for line in content.lines() {
        if !line.contains("; fully-qualified") {
            continue;
        }
        let Some(hash) = line.find('#') else { continue };
        let comment = line[hash + 1..].trim();
        let fields: Vec<&str> = comment.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        let emoji = fields[0];
        let name = fields[2..].join(" ");
        out.push_str(&format!("{} {}\t{}\n", emoji, name, emoji));
    }
    out
}

fn apply(selected: &str) -> String {
    if selected.is_empty() {
        return "ERR nothing selected".to_string();
    }
    // Defense in depth: the picker's on-select is shell-executed as
    // `emoji --apply '<value>'`. A substituted value containing a
    // quote, whitespace, or shell metacharacter must never reach the
    // shell. Valid emoji are single grapheme clusters without those.
    if selected.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '\'' | '"'
                    | '\\'
                    | ';'
                    | '&'
                    | '|'
                    | '$'
                    | '`'
                    | '\n'
                    | '\r'
                    | '('
                    | ')'
                    | '<'
                    | '>'
            )
    }) {
        return "ERR invalid emoji".to_string();
    }
    let copy_code = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/plain", selected, &[]);
    let (type_code, _out, type_err) = crate::proc::run(&wtype(), &[selected]);
    if copy_code != 0 && type_code != 0 {
        return format!("ERR clipboard and type failed ({})", type_err.trim());
    }
    "OK".to_string()
}

pub fn handle(args: &[String]) -> String {
    if args
        .iter()
        .any(|a| a == "-h" || a == "--help" || a == "help")
    {
        return "emoji — pick one, copy it, and type it\n\nUsage:\n  emoji\n  emoji --apply EMOJI"
            .to_string();
    }
    if args.first().map(|s| s.as_str()) == Some("--apply") {
        let selected = args.get(1..).unwrap_or(&[]).join(" ");
        return apply(selected.trim());
    }
    let Some(path) = emoji_test_path() else {
        return "ERR emoji list not found (set EMOJI_TEST)".to_string();
    };
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return format!("ERR {}", e),
    };
    let entries = parse_emoji_test(&content);
    if entries.is_empty() {
        return "ERR nothing to pick from".to_string();
    }
    crate::pick::handle_with_stdin(
        &[
            "--title".to_string(),
            "emoji".to_string(),
            "--on-select".to_string(),
            "emoji --apply '{}'".to_string(),
        ],
        &entries,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fully_qualified_only() {
        let content = concat!(
            "2601 ; fully-qualified # ☀ E1.0 cloud\n",
            "2601 ; minimally-qualified # ☀ E1.0 cloud\n",
            "1F600 ; fully-qualified # 😀 E1.0 grinning face\n",
            "garbage line\n",
        );
        let entries = parse_emoji_test(content);
        let lines: Vec<&str> = entries.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("☀"));
        assert!(lines[0].contains("\t☀"));
        assert!(lines[1].contains("grinning face"));
    }

    #[test]
    fn parse_ignores_short_comments() {
        let entries = parse_emoji_test("1F600 ; fully-qualified # 😀\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn help_returns_usage() {
        let out = handle(&["--help".to_string()]);
        assert!(out.contains("emoji"));
    }
}
