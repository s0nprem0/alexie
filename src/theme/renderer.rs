//! `@TOKEN@` template renderer (port of `lib/render.sh` in anixie).
//!
//! Pure logic: given a template string and a [`crate::theme::tokens`] substitution
//! map, produce the rendered string. Fails if any `@[A-Z][A-Z0-9_]*@`
//! placeholder remains, mirroring the shell's `grep -Eq` guard in
//! `theme-apps.sh`.
//!
//! This is deliberately separate from the legacy `{{…}}` `theme render`
//! command in `vars/commands.rs` (Material-style vars); both exist but
//! `@TOKEN@` is the anixie generation path.

use std::collections::BTreeMap;

/// Replace every `@TOKEN@`-style placeholder present in `subs`.
/// Longer placeholders win so `@BG_DARK@` is not clobbered by `@BG@`.
pub(crate) fn render_template(
    src: &str,
    subs: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut ordered: Vec<(&String, &String)> = subs.iter().collect();
    ordered.sort_by_key(|e| std::cmp::Reverse(e.0.len()));
    let mut out = src.to_string();
    for (placeholder, value) in ordered {
        if !placeholder.is_empty() {
            out = out.replace(placeholder.as_str(), value);
        }
    }
    if let Some(leftover) = find_leftover_placeholder(&out) {
        return Err(format!("unrendered placeholder {leftover}"));
    }
    Ok(out)
}

/// Return the first `@UPPER_SNAKE@` placeholder in `s`, if any.
/// Mirrors `grep -Eq '@[A-Z][A-Z0-9_]*@'`.
pub(crate) fn find_leftover_placeholder(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let start = i;
            i += 1;
            if i < bytes.len() && (bytes[i] as char).is_ascii_uppercase() {
                let body_start = i;
                while i < bytes.len() {
                    let c = bytes[i] as char;
                    if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                if i > body_start && i < bytes.len() && bytes[i] == b'@' {
                    return Some(s[start..=i].to_string());
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn replaces_all_forms() {
        let subs = map(&[
            ("@BG@", "#0f111a"),
            ("@BG_HEX@", "0f111a"),
            ("@PRIMARY_ALPHA_AA@", "#82aaffaa"),
        ]);
        let out = render_template("bg:@BG@ hex:@BG_HEX@ p:@PRIMARY_ALPHA_AA@", &subs).unwrap();
        assert_eq!(out, "bg:#0f111a hex:0f111a p:#82aaffaa");
    }

    #[test]
    fn longest_first_avoids_clobber() {
        let subs = map(&[("@BG@", "X"), ("@BG_DARK@", "Y")]);
        let out = render_template("@BG_DARK@/@BG@", &subs).unwrap();
        assert_eq!(out, "Y/X");
    }

    #[test]
    fn leftover_placeholder_errors() {
        let subs = map(&[("@BG@", "#000000")]);
        let err = render_template("x @BG@ y @MISSING@ z", &subs).unwrap_err();
        assert!(err.contains("@MISSING@"), "got: {err}");
    }

    #[test]
    fn no_placeholder_is_ok() {
        let out = render_template("plain text", &BTreeMap::new()).unwrap();
        assert_eq!(out, "plain text");
    }

    #[test]
    fn finder_ignores_lowercase() {
        assert_eq!(find_leftover_placeholder("a @b@ c"), None);
        assert!(find_leftover_placeholder("a @B_SOME@ c").is_some());
    }
}
