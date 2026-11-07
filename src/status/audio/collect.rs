//! Collect sinks/sources/apps/diagnostics/media.

use super::super::common::pactl;
use super::parse::{get_pactl_json, parse_pactl_info, parse_volume, sample_rate_of, sink_icon};
use super::sources::{get_all_media_sources, get_media_info_for, resolve_current_player};
use super::types::{AppInfo, AudioResult, Diagnostics, SinkInfo, SourceInfo};
pub(crate) fn collect() -> AudioResult {
    let (default_sink_name, default_source_name, pipewire_version) = {
        let out = {
            let (code, o, _) = crate::proc::run(&pactl(), &["info"]);
            if code == 0 { o } else { String::new() }
        };
        parse_pactl_info(&out)
    };

    // Sinks
    let sinks_raw = get_pactl_json("sinks");
    let mut sinks = Vec::new();
    let mut default_sink = None;

    if let Some(sinks_arr) = sinks_raw.as_array() {
        for s in sinks_arr {
            let vol = parse_volume(s.get("volume").unwrap_or(&serde_json::Value::Null));
            let props = s.get("properties").and_then(|p| p.as_object());
            let device_bus = props
                .and_then(|p| p.get("device.bus"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let is_bt = device_bus == "bluetooth";

            let sample_spec = s
                .get("sample_specification")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let sample_rate = sample_rate_of(sample_spec);

            let active_port = s
                .get("active_port")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let icon = sink_icon(is_bt, &active_port);

            let index = s.get("index").cloned().unwrap_or(serde_json::Value::Null);
            let name = s
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let description = s
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let muted = s.get("mute").and_then(|v| v.as_bool()).unwrap_or(false);

            let sink_info = SinkInfo {
                index,
                name: name.clone(),
                description,
                volume: vol,
                muted,
                is_bluetooth: is_bt,
                sample_rate,
                icon: icon.to_string(),
            };

            sinks.push(sink_info.clone());

            if name == default_sink_name {
                default_sink = Some(sink_info);
            }
        }
    }

    // Sources (skip monitors)
    let sources_raw = get_pactl_json("sources");
    let mut sources = Vec::new();
    let mut default_source = None;

    if let Some(sources_arr) = sources_raw.as_array() {
        for s in sources_arr {
            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if s.get("monitor_of_sink").is_some() || name.contains(".monitor") {
                continue;
            }

            let vol = parse_volume(s.get("volume").unwrap_or(&serde_json::Value::Null));
            let index = s.get("index").cloned().unwrap_or(serde_json::Value::Null);
            let description = s
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let muted = s.get("mute").and_then(|v| v.as_bool()).unwrap_or(false);

            let source_info = SourceInfo {
                index,
                name: name.to_string(),
                description,
                volume: vol,
                muted,
                icon: "audio-input-microphone".to_string(),
            };

            sources.push(source_info.clone());

            if name == default_source_name {
                default_source = Some(source_info);
            }
        }
    }

    // Apps (sink-inputs)
    let apps_raw = get_pactl_json("sink-inputs");
    let mut apps = Vec::new();

    if let Some(apps_arr) = apps_raw.as_array() {
        for a in apps_arr {
            let vol = parse_volume(a.get("volume").unwrap_or(&serde_json::Value::Null));
            let index = a.get("index").cloned().unwrap_or(serde_json::Value::Null);
            let muted = a.get("mute").and_then(|v| v.as_bool()).unwrap_or(false);

            let props = a.get("properties").and_then(|p| p.as_object());
            let app_name = props
                .and_then(|p| p.get("application.name").or_else(|| p.get("media.name")))
                .and_then(|v| v.as_str())
                .unwrap_or("Audio Stream")
                .to_string();

            apps.push(AppInfo {
                index,
                name: app_name,
                volume: vol,
                muted,
            });
        }
    }

    if default_sink.is_none() && !sinks.is_empty() {
        default_sink = Some(sinks[0].clone());
    }
    if default_source.is_none() && !sources.is_empty() {
        default_source = Some(sources[0].clone());
    }

    let diag_sr = default_sink
        .as_ref()
        .map(|s| s.sample_rate.clone())
        .unwrap_or_else(|| "48kHz".to_string());
    let mut diag_desc = default_sink
        .as_ref()
        .map(|s| s.description.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    diag_desc = diag_desc.replace("Analog Stereo", "").trim().to_string();

    let media_sources = get_all_media_sources();
    let current_player = resolve_current_player(&media_sources);
    let media = get_media_info_for(current_player.as_deref());

    AudioResult {
        default_sink,
        default_source,
        sinks,
        sources,
        apps,
        diagnostics: Diagnostics {
            pipewire_version,
            sample_rate: diag_sr,
            output_desc: diag_desc,
        },
        media,
        media_sources,
        current_media_source: current_player,
    }
}
