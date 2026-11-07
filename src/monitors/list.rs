use super::state::{Snapshot, state_file};
use serde_json::Value;

pub(crate) fn read_saved_map() -> std::collections::HashMap<String, (String, String)> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string(state_file()) {
        for line in content.lines() {
            if line.starts_with('#') {
                continue;
            }
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.is_empty() {
                continue;
            }
            let name = t[0].to_string();
            let position = t
                .get(5)
                .filter(|s| !s.is_empty())
                .unwrap_or(&"auto")
                .to_string();
            let mode = t
                .get(6)
                .filter(|s| !s.is_empty())
                .unwrap_or(&"preferred")
                .to_string();
            // normalize a saved disabled/mode that an older or broken port left
            let mode = mode.strip_suffix("Hz").unwrap_or(&mode).to_string();
            map.insert(name, (position, mode));
        }
    }
    map
}

pub(crate) fn list_action(snap: &Snapshot) -> String {
    let mut v = Value::Array(snap.all.clone());
    let arr = match v.as_array_mut() {
        Some(a) => a,
        None => return "{}".to_string(),
    };

    // id -> name
    let id_to_name = &snap.id_to_name;

    let saved = read_saved_map();

    for m in arr.iter_mut() {
        let name = m
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        // resolve mirrorOf (id -> name)
        let mirror_raw = m.get("mirrorOf").cloned().unwrap_or(Value::Null);
        let mut mirror = match mirror_raw {
            Value::String(s) => s,
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => {
                if b {
                    "true".to_string()
                } else {
                    "none".to_string()
                }
            }
            _ => "none".to_string(),
        };
        if mirror.is_empty() {
            mirror = "none".to_string();
        }
        if mirror != "none"
            && let Ok(id) = mirror.parse::<i64>()
            && let Some(n) = id_to_name.get(&id)
        {
            mirror = n.clone();
        }

        let (pos, mode) = saved
            .get(&name)
            .cloned()
            .unwrap_or_else(|| ("auto".to_string(), "preferred".to_string()));

        // availableModes: strip trailing Hz, unique
        let mut seen = std::collections::HashSet::new();
        let mut modes_out: Vec<Value> = Vec::new();
        if let Some(arr_modes) = m.get("availableModes").and_then(|x| x.as_array()) {
            for x in arr_modes {
                if let Some(s) = x.as_str() {
                    let s = s.strip_suffix("Hz").unwrap_or(s);
                    if seen.insert(s.to_string()) {
                        modes_out.push(Value::String(s.to_string()));
                    }
                }
            }
        }

        if let Some(obj) = m.as_object_mut() {
            obj.insert("mirrorOf".to_string(), Value::String(mirror));
            obj.insert("savedPosition".to_string(), Value::String(pos));
            obj.insert("savedMode".to_string(), Value::String(mode));
            obj.insert("availableModes".to_string(), Value::Array(modes_out));
        }
    }

    serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string())
}
