use std::env;
use std::fs;
use std::path::PathBuf;

pub fn config_path() -> PathBuf {
    let base = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.config", home)
    });
    PathBuf::from(base).join("alexie").join("config.toml")
}

pub fn load() -> Option<toml::Value> {
    let path = config_path();
    let content = fs::read_to_string(path).ok()?;
    // toml 1.x parses documents into a Table, not a bare Value.
    toml::from_str::<toml::Table>(&content)
        .ok()
        .map(toml::Value::Table)
}

pub fn get_str(keys: &[&str], default: &str) -> String {
    if let Some(v) = load() {
        let mut cur = &v;
        for k in keys {
            match cur.get(*k) {
                Some(next) => cur = next,
                None => return default.to_string(),
            }
        }
        if let Some(s) = cur.as_str() {
            return s.to_string();
        }
    }
    default.to_string()
}

pub fn get_i64(keys: &[&str], default: i64) -> i64 {
    if let Some(v) = load() {
        let mut cur = &v;
        for k in keys {
            match cur.get(*k) {
                Some(next) => cur = next,
                None => return default,
            }
        }
        if let Some(n) = cur.as_integer() {
            return n;
        }
    }
    default
}

pub fn get_bool(keys: &[&str], default: bool) -> bool {
    if let Some(v) = load() {
        let mut cur = &v;
        for k in keys {
            match cur.get(*k) {
                Some(next) => cur = next,
                None => return default,
            }
        }
        if let Some(b) = cur.as_bool() {
            return b;
        }
    }
    default
}

pub fn get_str_list(keys: &[&str]) -> Option<Vec<String>> {
    if let Some(v) = load() {
        let mut cur = &v;
        for k in keys {
            cur = cur.get(*k)?;
        }
        if let Some(arr) = cur.as_array() {
            return Some(
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect(),
            );
        }
    }
    None
}
