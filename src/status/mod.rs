//! Status: network/sysmon/audio/bluetooth, text or JSON.

pub mod audio;
pub mod bluetooth;
pub mod common;
pub mod network;
pub mod sysmon;

const SUBSYSTEMS: &[&str] = &["network", "sysmon", "audio", "bluetooth"];

pub fn usage() -> String {
    "status — system status (network/sysmon/audio/bluetooth) as text or JSON.\n\nUsage:\n  alexie status <network|sysmon|audio|bluetooth> [--json] [flags]\n  alexie status network [--rescan]\n  alexie status bluetooth [--scan]\n  alexie network|sysmon|audio|bluetooth [--json]\n".to_string()
}

pub(crate) fn summary() -> String {
    // Combined one-glance view: useful from a terminal, cheap enough
    // to shell out to the four collectors (each tolerates missing tools).
    let net = network::collect(false);
    let sys = sysmon::collect();
    let aud = audio::collect();
    let bt = bluetooth::collect(false);

    let mut out = String::new();
    out.push_str(&format!(
        "network wifi={} connected={} ssid={} signal={}%\n",
        if net.wifi_enabled { "on" } else { "off" },
        if net.connected { "yes" } else { "no" },
        if net.active_ssid.is_empty() {
            "-"
        } else {
            &net.active_ssid
        },
        net.active_signal
    ));
    out.push_str(&format!(
        "sysmon cpu={} cpu_usage={}% ram={:.1}/{:.1}GiB gpu={}\n",
        sys.cpu_name,
        sys.cpu_usage,
        sys.ram_used,
        sys.ram_total,
        if sys.gpu_name.is_empty() {
            "-"
        } else {
            &sys.gpu_name
        }
    ));
    out.push_str(&format!(
        "audio sink={} vol={}% sources={} sinks={}\n",
        aud.default_sink
            .as_ref()
            .map(|s| s.description.as_str())
            .unwrap_or("-"),
        aud.default_sink.as_ref().map(|s| s.volume).unwrap_or(0),
        aud.sources.len(),
        aud.sinks.len()
    ));
    out.push_str(&format!(
        "bluetooth enabled={} devices={}\n",
        if bt.enabled { "yes" } else { "no" },
        bt.devices.len()
    ));
    out.push_str("See `alexie status <network|sysmon|audio|bluetooth> --json` for full detail.\n");
    out.trim_end().to_string()
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    // Bare `alexie status` => combined summary (read-only default).
    if sub.is_empty() {
        return summary();
    }
    // `status --json` with no subsystem => JSON overview.
    if sub == "--json" {
        return summary_json();
    }
    match sub {
        "-h" | "--help" | "help" => usage(),
        "list" => SUBSYSTEMS.join("\n"),
        "network" => network::handle(&args[1..]),
        "sysmon" => sysmon::handle(&args[1..]),
        "audio" => audio::handle(&args[1..]),
        "bluetooth" => bluetooth::handle(&args[1..]),
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}

pub(crate) fn summary_json() -> String {
    let net = network::collect(false);
    let sys = sysmon::collect();
    let aud = audio::collect();
    let bt = bluetooth::collect(false);
    serde_json::json!({
        "network": net,
        "sysmon": sys,
        "audio": aud,
        "bluetooth": bt,
    })
    .to_string()
}

// D-Bus helpers.
pub(crate) fn network_json() -> String {
    network::handle(&["--json".to_string()])
}
pub(crate) fn sysmon_json() -> String {
    sysmon::handle(&["--json".to_string()])
}
pub(crate) fn audio_json() -> String {
    audio::handle(&["--json".to_string()])
}
pub(crate) fn bluetooth_json() -> String {
    bluetooth::handle(&["--json".to_string()])
}

// Aliases.
pub(crate) fn network_handle(args: &[String]) -> String {
    network::handle(args)
}
pub(crate) fn sysmon_handle(args: &[String]) -> String {
    sysmon::handle(args)
}
pub(crate) fn audio_handle(args: &[String]) -> String {
    audio::handle(args)
}
pub(crate) fn bluetooth_handle(args: &[String]) -> String {
    bluetooth::handle(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }

    #[test]
    fn list_names_subsystems() {
        let out = handle(&["list".to_string()]);
        for name in SUBSYSTEMS {
            assert!(out.contains(name), "list missing {name}: {out}");
        }
    }

    #[test]
    fn help_returns_usage() {
        let out = handle(&["--help".to_string()]);
        assert!(out.contains("alexie status"), "got: {out}");
    }

    #[test]
    fn summary_json_is_object_with_all_keys() {
        let out = summary_json();
        let v: serde_json::Value = serde_json::from_str(&out).expect("summary_json must be JSON");
        for k in SUBSYSTEMS {
            assert!(v.get(*k).is_some(), "missing {k}");
        }
    }

    #[test]
    fn alias_handles_delegate_to_modules() {
        // Alias `network` and `status network` share the same JSON shape.
        let a = network_handle(&["--json".to_string()]);
        let b = handle(&["network".to_string(), "--json".to_string()]);
        let va: serde_json::Value = serde_json::from_str(&a).unwrap();
        let vb: serde_json::Value = serde_json::from_str(&b).unwrap();
        for k in ["wifi_enabled", "connected", "networks", "vpns"] {
            assert!(va.get(k).is_some() && vb.get(k).is_some(), "missing {k}");
        }
    }
}
