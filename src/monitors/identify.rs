use super::state::{self, Snapshot};
use crate::proc::run;
use serde_json::{Map, Value};

pub(crate) fn identify_action(snap: &Snapshot, args: &[String]) -> String {
    let duration = match parse_identify_duration(args) {
        Ok(d) => d.to_string(),
        Err(e) => return e,
    };
    let payload = identify_descriptions(snap);
    let (_, out, err) = run(
        &state::qs(),
        &["ipc", "call", "identify", "flash", &duration, &payload],
    );
    if identify_reply_ok(&out) {
        return "OK".to_string();
    }
    let detail = out.trim();
    let detail = if detail.is_empty() {
        err.trim()
    } else {
        detail
    };
    let detail = if detail.is_empty() {
        "no reply"
    } else {
        detail
    };
    format!(
        "ERR could not ask the running shell to identify the displays ({})",
        detail
    )
}

// `ipc call` exits zero even when the target does not exist (it prints
// "Target not found."), so the answer — not the exit code — is what says
// whether the shell is new enough to have the handler.
pub(crate) fn identify_reply_ok(out: &str) -> bool {
    matches!(out.trim(), "ok" | "\"ok\"")
}

// Milliseconds to keep the overlay up. The shell defaults and clamps too;
// parsing here only rejects a non-numeric argument early.
pub(crate) fn parse_identify_duration(args: &[String]) -> Result<i64, String> {
    let Some(raw) = args.get(1) else {
        return Ok(3000);
    };
    match raw.parse::<i64>() {
        Ok(ms) => Ok(ms.clamp(500, 10_000)),
        Err(_) => Err("ERR usage: monitors identify [MS]".to_string()),
    }
}

// Friendly "make model" per output, for the secondary line under the name.
// The hyprctl description appends the serial, which is not what a bezel says.
pub(crate) fn identify_descriptions(snap: &Snapshot) -> String {
    let mut map = Map::new();
    for m in &snap.all {
        let Some(name) = m.get("name").and_then(|x| x.as_str()) else {
            continue;
        };
        let make = m.get("make").and_then(|x| x.as_str()).unwrap_or("").trim();
        let model = m.get("model").and_then(|x| x.as_str()).unwrap_or("").trim();
        let label = format!("{} {}", make, model).trim().to_string();
        let label = if label.is_empty() {
            m.get("description")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            label
        };
        if !label.is_empty() {
            map.insert(name.to_string(), Value::String(label));
        }
    }
    serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn identities_action(snap: &Snapshot) -> String {
    let arr = &snap.all;
    let mut obj = Map::new();
    for m in arr {
        let Some(name) = m.get("name").and_then(|x| x.as_str()) else {
            continue;
        };
        let mut inner = Map::new();
        for key in ["make", "model", "serial", "description"] {
            if let Some(val) = m.get(key) {
                inner.insert(key.to_string(), val.clone());
            }
        }
        obj.insert(name.to_string(), Value::Object(inner));
    }
    serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn sync_identities_action(snap: &Snapshot) -> String {
    let out = identities_action(snap);
    let _ = std::fs::create_dir_all(state::state_dir());
    let _ = std::fs::write(
        state::state_dir().join("monitor_identities.json"),
        format!("{}\n", out.trim()),
    );
    "identities saved".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_reply_only_ok_succeeds() {
        assert!(identify_reply_ok("ok\n"));
        assert!(identify_reply_ok("\"ok\""));
        assert!(!identify_reply_ok("Target not found."));
        assert!(!identify_reply_ok(""));
        assert!(!identify_reply_ok("error"));
    }

    #[test]
    fn identify_duration_defaults_clamps_and_rejects() {
        assert_eq!(parse_identify_duration(&["identify".to_string()]), Ok(3000));
        assert_eq!(
            parse_identify_duration(&["identify".to_string(), "5000".to_string()]),
            Ok(5000)
        );
        assert_eq!(
            parse_identify_duration(&["identify".to_string(), "10".to_string()]),
            Ok(500)
        );
        assert_eq!(
            parse_identify_duration(&["identify".to_string(), "99999".to_string()]),
            Ok(10_000)
        );
        assert!(parse_identify_duration(&["identify".to_string(), "soon".to_string()]).is_err());
    }

    #[test]
    fn identify_descriptions_prefers_make_model_over_serial() {
        let snap = Snapshot {
            all: vec![
                serde_json::json!({"name": "HDMI-A-1", "make": "GWD", "model": "ARZOPA", "serial": "0000001083904", "description": "GWD ARZOPA 0000001083904"}),
                serde_json::json!({"name": "eDP-1", "description": "Chimei Innolux Corporation 0x15E7"}),
                serde_json::json!({"name": "DP-1"}),
            ],
            by_name: std::collections::HashMap::new(),
            id_to_name: std::collections::HashMap::new(),
        };
        let payload: serde_json::Value =
            serde_json::from_str(&identify_descriptions(&snap)).unwrap();
        assert_eq!(payload["HDMI-A-1"], "GWD ARZOPA");
        assert_eq!(payload["eDP-1"], "Chimei Innolux Corporation 0x15E7");
        assert!(payload.get("DP-1").is_none());
    }
}
