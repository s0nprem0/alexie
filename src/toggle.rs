use crate::proc::run;

fn hyprctl_temp() -> String {
    let (code, out, _) = run("hyprctl", &["hyprsunset", "temperature"]);
    if code == 0 {
        out.trim().to_string()
    } else {
        String::new()
    }
}

fn nightlight_state() -> &'static str {
    let t = hyprctl_temp();
    if !t.is_empty()
        && let Ok(n) = t.parse::<i64>()
        && n < 6000
    {
        return "on";
    }
    "off"
}

fn nightlight_toggle() {
    if nightlight_state() == "on" {
        let _ = run("hyprctl", &["hyprsunset", "temperature", "6500"]);
    } else {
        let _ = run("hyprctl", &["hyprsunset", "temperature", "4000"]);
    }
    let t = hyprctl_temp();
    let _ = run(
        "quickshell",
        &[
            "ipc",
            "call",
            "indicator",
            "nightlight",
            &if t.is_empty() { "0".to_string() } else { t },
        ],
    );
}

fn idle_state() -> &'static str {
    let (code, out, _) = run("quickshell", &["ipc", "call", "idle", "status"]);
    if code == 0 && out.trim() == "on" {
        "on"
    } else {
        "off"
    }
}

fn idle_toggle() {
    let _ = run("quickshell", &["ipc", "call", "idle", "toggle"]);
}

fn dnd_state() -> &'static str {
    let (code, out, _) = run("quickshell", &["ipc", "call", "notify", "status"]);
    if code == 0 && out.trim() == "on" {
        "on"
    } else {
        "off"
    }
}

fn dnd_toggle() {
    let _ = run("quickshell", &["ipc", "call", "notify", "toggle"]);
}

fn stayawake_state() -> &'static str {
    let (code, _, _) = run(
        "systemctl",
        &["--user", "is-active", "--quiet", "anixie-stayawake"],
    );
    if code == 0 { "on" } else { "off" }
}

fn stayawake_toggle() {
    if stayawake_state() == "on" {
        let _ = run("systemctl", &["--user", "stop", "anixie-stayawake"]);
    } else {
        let _ = run("systemctl", &["--user", "start", "anixie-stayawake"]);
    }
    let _ = run(
        "quickshell",
        &["ipc", "call", "indicator", "stayawake", stayawake_state()],
    );
}

fn label(name: &str) -> &'static str {
    match name {
        "nightlight" => "Night light",
        "idle" => "Idle lock",
        "dnd" => "Silence notifications",
        "stayawake" => "Stay awake",
        _ => "Unknown",
    }
}

pub fn label_pub(name: &str) -> &'static str {
    label(name)
}

pub fn is_toggle(name: &str) -> bool {
    matches!(name, "nightlight" | "idle" | "dnd" | "stayawake")
}

fn state_of(name: &str) -> String {
    match name {
        "nightlight" => nightlight_state().to_string(),
        "idle" => idle_state().to_string(),
        "dnd" => dnd_state().to_string(),
        "stayawake" => stayawake_state().to_string(),
        _ => "ERR no toggle".to_string(),
    }
}

pub fn state_pub(name: &str) -> String {
    state_of(name)
}

fn toggle_one(name: &str) {
    match name {
        "nightlight" => nightlight_toggle(),
        "idle" => idle_toggle(),
        "dnd" => dnd_toggle(),
        "stayawake" => stayawake_toggle(),
        _ => {}
    }
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "" | "show" | "status" => {
            if args.len() >= 2 {
                return state_of(&args[1]);
            }
            let mut out = String::new();
            for name in ["nightlight", "idle", "dnd", "stayawake"] {
                out.push_str(&format!("{} {}\n", name, state_of(name)));
            }
            out.trim_end().to_string()
        }
        "list" => {
            let rows: Vec<Vec<String>> = ["nightlight", "idle", "dnd", "stayawake"]
                .iter()
                .map(|name| vec![label(name).to_string(), state_of(name)])
                .collect();
            crate::table::render(&["Toggle", "State"], &rows)
        }
        "nightlight" | "idle" | "dnd" | "stayawake" => {
            toggle_one(sub);
            format!("{}: {}", label(sub), state_of(sub))
        }
        _ => {
            // Fully ported (no toggle-legacy): running `toggle` here
            // would re-enter alexie via the toggle shim and recurse.
            // There is also no `automation` subcommand in legacy
            // toggle.sh — the old arm returned an empty string.
            // Match legacy: unknown -> error.
            format!("ERR unknown subcommand: {}", sub)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
        // `automation` was a bogus arm returning ""; it is unknown now.
        let out = handle(&["automation".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }

    #[test]
    fn label_and_state_of_unknown() {
        assert_eq!(label("nope"), "Unknown");
        assert!(state_of("nope").starts_with("ERR"));
    }
}
