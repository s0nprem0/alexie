//! Audio status (wabi `get_audio_status` JSON).
//!
//! Split from a single 550-line `audio.rs`:
//! - `types` — SinkInfo/SourceInfo/AppInfo/Diagnostics/MediaInfo/MediaSource/AudioResult
//! - `parse` — pactl parsing: volume, sample rate, icons
//! - `collect` — sinks/sources/apps/diagnostics/media collection
//! - `sources` — media sources via playerctl
//!
//! `render_text()`/`handle()` below just orchestrate.

pub(crate) mod types;
pub(crate) mod parse;
pub(crate) mod collect;
pub(crate) mod sources;

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
        out.push_str(&format!("source {}  {}%{}\n", s.description, s.volume, if s.muted { " (muted)" } else { "" }));
    }
    if !r.sinks.is_empty() {
        let rows: Vec<Vec<String>> = r
            .sinks
            .iter()
            .map(|s| vec![s.description.clone(), format!("{}%", s.volume), if s.muted { "yes".into() } else { "no".into() }])
            .collect();
        out.push_str(&crate::table::render(&["Sink", "Volume", "Muted"], &rows));
        out.push('\n');
    }
    if !r.apps.is_empty() {
        let rows: Vec<Vec<String>> = r
            .apps
            .iter()
            .map(|a| vec![a.name.clone(), format!("{}%", a.volume), if a.muted { "yes".into() } else { "no".into() }])
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
