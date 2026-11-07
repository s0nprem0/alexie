use super::state::{atomic_write, state_dir};

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct Pending {
    pub(crate) output: String,
    pub(crate) key: String,
    pub(crate) old_value: String,
    pub(crate) new_value: String,
}

pub(crate) fn pending_lock() -> Option<std::fs::File> {
    let dir = state_dir();
    let _ = std::fs::create_dir_all(&dir);
    let lock_path = dir.join("display-pending.lock");
    // non-blocking try + short retry
    for _ in 0..20 {
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&lock_path)
        {
            Ok(f) => return Some(f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(_) => return None,
        }
    }
    None
}

pub(crate) fn remove_pending(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let lock = state_dir().join("display-pending.lock");
    let _ = std::fs::remove_file(&lock);
}

pub(crate) fn read_pending() -> Option<Pending> {
    let path = state_dir().join("display-pending");
    let content = std::fs::read_to_string(&path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(p) = serde_json::from_str::<Pending>(trimmed) {
        return Some(p);
    }
    // legacy TSV fallback
    let mut parts = trimmed.split('\t');
    let output = parts.next()?.to_string();
    let key = parts.next()?.to_string();
    let old_value = parts.next()?.to_string();
    let new_value = parts.next()?.to_string();
    if key == "mode" {
        let old_value = old_value
            .strip_suffix("Hz")
            .unwrap_or(&old_value)
            .to_string();
        let new_value = new_value
            .strip_suffix("Hz")
            .unwrap_or(&new_value)
            .to_string();
        return Some(Pending {
            output,
            key,
            old_value,
            new_value,
        });
    }
    Some(Pending {
        output,
        key,
        old_value,
        new_value,
    })
}

pub(crate) fn write_pending(
    output: &str,
    key: &str,
    old_value: &str,
    new_value: &str,
) -> std::io::Result<()> {
    let _lock = pending_lock();
    let p = Pending {
        output: output.to_string(),
        key: key.to_string(),
        old_value: old_value.to_string(),
        new_value: new_value.to_string(),
    };
    let json = serde_json::to_string(&p).unwrap_or_else(|_| "{}".to_string());
    let path = state_dir().join("display-pending");
    atomic_write(&path, &format!("{}\n", json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_json_roundtrip() {
        let p = Pending {
            output: "eDP-1".into(),
            key: "scale".into(),
            old_value: "1".into(),
            new_value: "1.5".into(),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: Pending = serde_json::from_str(&s).unwrap();
        assert_eq!(back.output, "eDP-1");
        assert_eq!(back.new_value, "1.5");
        // legacy TSV fallback
        let tsv = "eDP-1\tscale\t1\t1.5";
        let mut parts = tsv.split('\t');
        assert_eq!(parts.next().unwrap(), "eDP-1");
    }
}
