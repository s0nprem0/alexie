//! Tool resolution: PATH search without shell-outs, preferring
//! `name-legacy` so delegation never recurses through shims, plus
//! shared env-overridable binary resolvers (wl-copy, quickshell,
//! notify-send) so callers don't each redefine the same one-liner.

pub fn find_in_path(name: &str, path_str: &str) -> Option<std::path::PathBuf> {
    // Names containing a path separator are used as-is: if the file
    // exists, that is the binary; otherwise Command::new will fail.
    if name.contains('/') {
        let p = std::path::PathBuf::from(name);
        return if is_executable(&p) { Some(p) } else { None };
    }
    for dir in path_str.split(':') {
        if dir.is_empty() {
            continue;
        }
        let p = std::path::Path::new(dir).join(name);
        if is_executable(&p) {
            return Some(p);
        }
    }
    None
}

/// Env-overridable binary: `$ENV_NAME` or `default`.
pub fn tool(env_name: &str, default: &str) -> String {
    std::env::var(env_name).unwrap_or_else(|_| default.to_string())
}

pub fn wl_copy() -> String { tool("WL_COPY", "wl-copy") }

pub fn quickshell() -> String { tool("QUICKSHELL", "quickshell") }

pub fn notify_send() -> String { tool("NOTIFY_SEND", "notify-send") }

fn is_executable(p: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

pub fn resolve_tool(name: &str) -> String {
    // Prefer `name-legacy` so delegation never recurses through an
    // alexie shim, falling back to `name` itself. The old version
    // spawned `sh -c "command -v …"` twice on *every* external call
    // (hyprctl, date, jq, …), which dominates the cost of short
    // commands like status queries.
    let path = std::env::var("PATH").unwrap_or_default();
    let legacy = format!("{}-legacy", name);
    if find_in_path(&legacy, &path).is_some() {
        return legacy;
    }
    if find_in_path(name, &path).is_some() {
        return name.to_string();
    }
    name.to_string()
}

