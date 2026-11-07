//! Audio status (wabi `get_audio_status` JSON).
//!
//! Split from a single 550-line `audio.rs`:
//! - `types` — SinkInfo/SourceInfo/AppInfo/Diagnostics/MediaInfo/MediaSource/AudioResult
//! - `parse` — pactl parsing: volume, sample rate, icons
//! - `collect` — sinks/sources/apps/diagnostics/media collection
//! - `sources` — media sources via playerctl
//!
//! `render_text()`/`handle()` below just orchestrate.

pub(crate) mod collect;
pub(crate) mod parse;
pub(crate) mod sources;
pub(crate) mod types;

#[allow(unused_imports)] // re-exported for public API compat
pub use types::{AppInfo, AudioResult, Diagnostics, MediaInfo, MediaSource, SinkInfo, SourceInfo};

use super::common::print_json;
pub(crate) use collect::collect;
#[cfg(test)]
use parse::{parse_pactl_info, parse_volume, sample_rate_of};

pub(crate) fn render_text(r: &AudioResult) -> String {
    let mut out = String::new();
    if let Some(s) = &r.default_sink {
        out.push_str(&format!(
            "sink {}  {}%{}  {}\n",
            s.description,
            s.volume,
            if s.muted { " (muted)" } else { "" },
            s.sample_rate
        ));
    } else {
        out.push_str("sink none\n");
    }
    if let Some(s) = &r.default_source {
        out.push_str(&format!(
            "source {}  {}%{}\n",
            s.description,
            s.volume,
            if s.muted { " (muted)" } else { "" }
        ));
    }
    if !r.sinks.is_empty() {
        let rows: Vec<Vec<String>> = r
            .sinks
            .iter()
            .map(|s| {
                vec![
                    s.description.clone(),
                    format!("{}%", s.volume),
                    if s.muted { "yes".into() } else { "no".into() },
                ]
            })
            .collect();
        out.push_str(&crate::table::render(&["Sink", "Volume", "Muted"], &rows));
        out.push('\n');
    }
    if !r.apps.is_empty() {
        let rows: Vec<Vec<String>> = r
            .apps
            .iter()
            .map(|a| {
                vec![
                    a.name.clone(),
                    format!("{}%", a.volume),
                    if a.muted { "yes".into() } else { "no".into() },
                ]
            })
            .collect();
        out.push_str(&crate::table::render(&["App", "Volume", "Muted"], &rows));
        out.push('\n');
    }
    if let Some(m) = &r.media {
        out.push_str(&format!("media {}: {} ({})", m.player, m.title, m.status));
    }
    out.trim_end().to_string()
}

pub(crate) fn handle(args: &[String]) -> String {
    // `alexie audio set <node-id> <sink-name>` — point the default output at a
    // sink and take the streams that are already playing with it (port of
    // anixie `audio-output-set`). Must be checked before the --json/flag
    // validation below, which is for the read-only status path.
    if !args.is_empty() && args[0] == "set" {
        return handle_set(&args[1..]);
    }

    let want_json = args.iter().any(|a| a == "--json");
    for a in args {
        if a.starts_with("--") && a != "--json" {
            return format!("ERR unknown flag: {}", a);
        }
    }
    let result = collect();
    if want_json {
        print_json(&result)
    } else {
        render_text(&result)
    }
}

/// Point the default output at a sink and take the streams along.
fn handle_set(args: &[String]) -> String {
    if args.len() != 2 {
        return "ERR usage: audio set <node-id> <sink-name>".to_string();
    }
    let node_id = &args[0];
    let sink_name = &args[1];
    if node_id.trim().is_empty() || sink_name.trim().is_empty() {
        return "ERR audio set needs a node id and a sink name".to_string();
    }

    let wpctl = crate::status::common::wpctl();
    let pactl = crate::status::common::pactl();

    // PipeWire and WirePlumber select by node id; the PulseAudio compat
    // layer names sinks, so both are set.
    let (code, _, err) = crate::proc::run(&wpctl, &["set-default", node_id]);
    if code != 0 {
        return format!("ERR wpctl set-default: {}", err.trim());
    }
    // Best-effort: the compat layer may be absent, and the default above is
    // what takes effect for new streams.
    let _ = crate::proc::run(&pactl, &["set-default-sink", sink_name]);

    // Move only real application streams. A filter-chain's own output carries
    // no application.name, and moving it would rewire processing.
    let (code, out, _) = crate::proc::run(&pactl, &["-f", "json", "list", "sink-inputs"]);
    if code == 0 {
        if let Ok(inputs) = serde_json::from_str::<serde_json::Value>(&out) {
            for input in inputs.as_array().cloned().unwrap_or_default() {
                let name = input
                    .pointer("/properties/application.name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if name.is_empty() || name == "EasyEffects" {
                    continue;
                }
                if let Some(index) = input.get("index").and_then(|v| v.as_u64()) {
                    let _ =
                        crate::proc::run(&pactl, &["move-sink-input", &index.to_string(), sink_name]);
                }
            }
        }
    }

    "OK".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_parses_first_channel() {
        let v = serde_json::json!({
            "front-left": {"value_percent": "65%"},
            "front-right": {"value_percent": "65%"}
        });
        assert_eq!(parse_volume(&v), 65);
        assert_eq!(parse_volume(&serde_json::Value::Null), 0);
    }

    #[test]
    fn sample_rate_parses() {
        assert_eq!(sample_rate_of("s16le 2ch 48000Hz"), "48kHz");
        assert_eq!(sample_rate_of("junk"), "48kHz");
    }

    #[test]
    fn pactl_info_parses_defaults() {
        let out = "Default Sink: alsa_output.pci-0000_04_00.5.analog-stereo\nDefault Source: alsa_input.pci-0000_04_00.5.analog-stereo\nServer Name: PulseAudio (from PipeWire 0.3.80)\n";
        let (sink, source, ver) = parse_pactl_info(out);
        assert!(sink.contains("alsa_output"));
        assert!(source.contains("alsa_input"));
        assert_eq!(ver, "from PipeWire 0.3.80");
    }

    #[test]
    fn handle_json_is_parseable() {
        let out = handle(&["--json".to_string()]);
        let v: serde_json::Value = serde_json::from_str(&out).expect("audio --json must be JSON");
        assert!(v.get("sinks").is_some());
        assert!(v.get("diagnostics").is_some());
    }

    #[test]
    fn handle_unknown_flag_fails_fast() {
        let out = handle(&["--bogus".to_string()]);
        assert!(out.starts_with("ERR unknown flag:"));
    }
}
