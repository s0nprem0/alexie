//! Token spec + substitution map (port of `lib/render.sh` in anixie).
//!
//! `tokens.json` maps palette keys (`background`) to placeholder names
//! (`BG`) plus derived ANSI tokens and alpha variants. Given a resolved
//! palette + extras this produces the `@TOKEN@ -> value` map the renderer
//! uses. It is pure logic — no IO beyond loading the spec.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) const EMBEDDED_TOKENS_JSON: &str = include_str!("../../tokens.json");

#[derive(Debug, Clone)]
pub(crate) struct TokenSpec {
    /// palette key -> TOKEN name, e.g. `background -> BG`
    pub colors: BTreeMap<String, String>,
    /// derived palette key (`ansi0`) -> source palette key (`background`)
    pub derived: BTreeMap<String, String>,
    pub alphas: Vec<String>,
}

impl TokenSpec {
    pub(crate) fn from_json(text: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if !v.is_object() {
            return Err("tokens.json is not an object".to_string());
        }
        fn str_map(v: &serde_json::Value, key: &str) -> BTreeMap<String, String> {
            let mut out = BTreeMap::new();
            if let Some(o) = v.get(key).and_then(|x| x.as_object()) {
                for (k, val) in o {
                    if let Some(s) = val.as_str() {
                        out.insert(k.clone(), s.to_string());
                    }
                }
            }
            out
        }
        let colors = str_map(&v, "colors");
        if colors.is_empty() {
            return Err("tokens.json missing colors".to_string());
        }
        let derived = str_map(&v, "derived");
        let alphas = v
            .get("alphas")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        Ok(Self { colors, derived, alphas })
    }

    pub(crate) fn embedded() -> Self {
        Self::from_json(EMBEDDED_TOKENS_JSON).expect("embedded tokens.json must be valid")
    }
}

/// Resolve a `tokens.json` path: `THEME_TOKENS` env → `theme.tokens_json`
/// config → `None` (callers fall back to [`TokenSpec::embedded`]).
#[allow(dead_code)]
pub(crate) fn tokens_path() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("THEME_TOKENS")
        && !v.trim().is_empty()
    {
        return Some(PathBuf::from(v));
    }
    let v = crate::config::get_str(&["theme", "tokens_json"], "");
    if v.trim().is_empty() { None } else { Some(PathBuf::from(v)) }
}

#[allow(dead_code)]
pub(crate) fn load_spec() -> TokenSpec {
    if let Some(path) = tokens_path()
        && let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(spec) = TokenSpec::from_json(&text)
    {
        return spec;
    }
    TokenSpec::embedded()
}

fn token_name(spec: &TokenSpec, key: &str) -> Option<String> {
    if let Some(t) = spec.colors.get(key) {
        Some(t.clone())
    } else if spec.derived.contains_key(key) {
        let n = key.strip_prefix("ansi").unwrap_or(key);
        Some(format!("ANSI_{n}"))
    } else {
        None
    }
}

fn palette_value(palette: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    palette
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Build the `@TOKEN@ -> value` map. Keys include the `@…@` delimiters;
/// extras (`HELIX_PARENT` etc.) are inserted verbatim as `@KEY@`.
pub(crate) fn substitution_map(
    palette: &serde_json::Map<String, serde_json::Value>,
    spec: &TokenSpec,
    extras: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut emit = |key: &str, token: &str, value: String| {
        out.insert(format!("@{token}@"), value.clone());
        let hex = value.strip_prefix('#').unwrap_or(&value).to_string();
        out.insert(format!("@{token}_HEX@"), hex.clone());
        for alpha in &spec.alphas {
            let lower = alpha.to_ascii_lowercase();
            out.insert(format!("@{token}_ALPHA_{alpha}@"), format!("#{}{}", hex, lower));
            out.insert(format!("@{token}_BARE_{alpha}@"), format!("{}{}", hex, lower));
            out.insert(format!("@{token}_RGBA_{alpha}@"), format!("rgba({}{})", hex, lower));
        }
        let _ = key;
    };

    // Base colors: palette key -> token.
    for key in spec.colors.keys() {
        let token = match token_name(spec, key) { Some(t) => t, None => continue };
        let value = match palette_value(palette, key) {
            Some(v) => v,
            None => {
                // Derived fallback: a derived key may source from a base key
                // (e.g. `ansi0 -> background`). Spec `derived` maps derived->source,
                // handled below; here `key` is a base key itself.
                continue;
            }
        };
        emit(key, &token, value);
    }
    // Derived (ANSI) tokens: derived key -> source palette key.
    for (dkey, src) in &spec.derived {
        let token = match token_name(spec, dkey) { Some(t) => t, None => continue };
        // Prefer an explicit palette value for the derived key, else the source.
        let value = palette_value(palette, dkey).or_else(|| palette_value(palette, src));
        if let Some(v) = value {
            emit(dkey, &token, v);
        }
    }
    for (k, v) in extras {
        out.insert(format!("@{k}@"), v.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TokenSpec {
        TokenSpec::from_json(
            r##"{"colors":{"background":"BG","primary":"PRIMARY"},"derived":{"ansi4":"primary"},"alphas":["FF","AA"]}"##,
        )
        .unwrap()
    }

    fn palette() -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(r##"{"background":"#0f111a","primary":"#82aaff"}"##).unwrap()
    }

    #[test]
    fn substitution_map_expands_all_forms() {
        let m = substitution_map(&palette(), &spec(), &BTreeMap::new());
        assert_eq!(m.get("@BG@").map(String::as_str), Some("#0f111a"));
        assert_eq!(m.get("@BG_HEX@").map(String::as_str), Some("0f111a"));
        assert_eq!(m.get("@BG_ALPHA_AA@").map(String::as_str), Some("#0f111aaa"));
        assert_eq!(m.get("@BG_BARE_AA@").map(String::as_str), Some("0f111aaa"));
        assert_eq!(m.get("@BG_RGBA_AA@").map(String::as_str), Some("rgba(0f111aaa)"));
        // derived ANSI_4 from primary
        assert_eq!(m.get("@ANSI_4@").map(String::as_str), Some("#82aaff"));
    }

    #[test]
    fn extras_passthrough() {
        let extras = BTreeMap::from([("HELIX_PARENT".to_string(), "base16_default_dark".to_string())]);
        let m = substitution_map(&palette(), &spec(), &extras);
        assert_eq!(m.get("@HELIX_PARENT@").map(String::as_str), Some("base16_default_dark"));
    }

    #[test]
    fn embedded_tokens_load() {
        let s = TokenSpec::embedded();
        assert!(s.colors.contains_key("background"));
        assert_eq!(s.colors.get("background").map(String::as_str), Some("BG"));
    }
}
