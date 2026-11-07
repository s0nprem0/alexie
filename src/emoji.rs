//! Emoji picker: pick one and type it via wtype.

use std::env;
use std::fs;
use std::path::PathBuf;

fn emoji_test_path() -> Option<PathBuf> {
    if let Ok(v) = env::var("EMOJI_TEST")
        && !v.is_empty() {
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
        && !v.is_empty() {
            return v;
        }
    let cfg = crate::config::get_str(&["emoji", "wtype"], "");
    if !cfg.is_empty() {
        return cfg;
    }
    "wtype".to_string()
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

pub fn handle(args: &[String]) -> String {
    if args.iter().any(|a| a == "-h" || a == "--help" || a == "help") {
        return "emoji — pick one and type it\n\nUsage:\n  emoji".to_string();
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
            format!("{} {{}}", wtype()),
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
