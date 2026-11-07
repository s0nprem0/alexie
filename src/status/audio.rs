//! Audio status (wabi `get_audio_status` JSON).

use super::common::{parse_percent, print_json, pactl, playerctl};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn current_player_path() -> PathBuf {
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(rt).join("quickshell_current_media_player")
}

fn load_current_player() -> Option<String> {
    fs::read_to_string(current_player_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[allow(dead_code)]
fn save_current_player(name: &str) {
    let _ = fs::write(current_player_path(), name);
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SinkInfo {
    pub index: serde_json::Value,
    pub name: String,
    pub description: String,
    pub volume: i64,
    pub muted: bool,
    pub is_bluetooth: bool,
    pub sample_rate: String,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SourceInfo {
    pub index: serde_json::Value,
    pub name: String,
    pub description: String,
    pub volume: i64,
    pub muted: bool,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub index: serde_json::Value,
    pub name: String,
    pub volume: i64,
    pub muted: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Diagnostics {
    pub pipewire_version: String,
    pub sample_rate: String,
    pub output_desc: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaInfo {
    pub player: String,
    pub title: String,
    pub artist: String,
    pub art_url: String,
    pub status: String,
    pub position: f64,
    pub length: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaSource {
    pub name: String,
    pub status: String,
    pub title: String,
    pub artist: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AudioResult {
    pub default_sink: Option<SinkInfo>,
    pub default_source: Option<SourceInfo>,
    pub sinks: Vec<SinkInfo>,
    pub sources: Vec<SourceInfo>,
    pub apps: Vec<AppInfo>,
    pub diagnostics: Diagnostics,
    pub media: Option<MediaInfo>,
    pub media_sources: Vec<MediaSource>,
    pub current_media_source: Option<String>,
}

fn get_pactl_json(category: &str) -> serde_json::Value {
    let (code, out, _) = crate::proc::run(&pactl(), &["-f", "json", "list", category]);
    if code != 0 {
        return serde_json::Value::Array(Vec::new());
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&out) {
        if val.is_array() {
            return val;
        } else if val.is_object() {
            return serde_json::Value::Array(vec![val]);
        }
    }
    serde_json::Value::Array(Vec::new())
}

pub(crate) fn parse_volume(vol_obj: &serde_json::Value) -> i64 {
    if let Some(obj) = vol_obj.as_object() {
        for (_chan, val_dict) in obj {
            if let Some(val_str) = val_dict.get("value_percent").and_then(|v| v.as_str())
                && let Some(vol) = parse_percent(val_str)
            {
                return vol;
            }
        }
    }
    0
}

pub(crate) fn sample_rate_of(sample_spec: &str) -> String {
    if let Some(hz_pos) = sample_spec.find("Hz") {
        let prefix = &sample_spec[..hz_pos];
        let digits: String = prefix
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let digits: String = digits.chars().rev().collect();
        if let Ok(hz) = digits.parse::<i64>() {
            return format!("{}kHz", hz / 1000);
        }
    }
    "48kHz".to_string()
}

pub(crate) fn sink_icon(is_bt: bool, active_port: &str) -> &'static str {
    if is_bt {
        "audio-headphones-bluetooth"
    } else if active_port.contains("headphone") {
        "audio-headphones"
    } else {
        "audio-speakers"
    }
}

fn parse_pactl_info(out: &str) -> (String, String, String) {
    let mut default_sink_name = String::new();
    let mut default_source_name = String::new();
    let mut pipewire_version = "Running".to_string();
    for line in out.lines() {
        if let Some(suffix) = line.strip_prefix("Default Sink:") {
            default_sink_name = suffix.trim().to_string();
        } else if let Some(suffix) = line.strip_prefix("Default Source:") {
            default_source_name = suffix.trim().to_string();
        } else if let Some(suffix) = line.strip_prefix("Server Name:") {
            let server_name = suffix.trim();
            if let Some(ver) = server_name.find('(').and_then(|start| {
                server_name.find(')').and_then(|end| {
                    if end > start {
                        Some(server_name[start + 1..end].to_string())
                    } else {
                        None
                    }
                })
            }) {
                pipewire_version = ver;
            }
        }
    }
    (default_sink_name, default_source_name, pipewire_version)
}

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

fn get_all_media_sources() -> Vec<MediaSource> {
    let players_str = {
        let (code, out, _) = crate::proc::run(&playerctl(), &["-l"]);
        if code == 0 { out } else { return Vec::new(); }
    };
    let mut sources: Vec<MediaSource> = Vec::new();
    for line in players_str.lines() {
        let player = line.trim();
        if player.is_empty() {
            continue;
        }
        let status = {
            let (code, out, _) = crate::proc::run(&playerctl(), &["--player", player, "status"]);
            if code == 0 { out.trim().to_string() } else { "Stopped".to_string() }
        };
        let title = playerctl_metadata(player, "{{title}}").unwrap_or_default();
        let artist = playerctl_metadata(player, "{{artist}}").unwrap_or_default();
        sources.push(MediaSource {
            name: player.to_string(),
            status,
            title,
            artist,
        });
    }
    sources
}

pub(crate) fn resolve_current_player(sources: &[MediaSource]) -> Option<String> {
    if sources.is_empty() {
        return None;
    }
    let stored = load_current_player();
    if let Some(ref name) = stored
        && sources.iter().any(|s| &s.name == name)
    {
        return Some(name.clone());
    }
    if let Some(playing) = sources.iter().find(|s| s.status == "Playing") {
        return Some(playing.name.clone());
    }
    if let Some(paused) = sources.iter().find(|s| s.status == "Paused") {
        return Some(paused.name.clone());
    }
    Some(sources[0].name.clone())
}

fn playerctl_metadata(player: &str, format: &str) -> Option<String> {
    let (code, out, _) = crate::proc::run(&playerctl(), &["--player", player, "metadata", "--format", format]);
    if code == 0 {
        Some(out.trim().to_string())
    } else {
        None
    }
}

fn playerctl_position(player: &str) -> f64 {
    let (code, out, _) = crate::proc::run(&playerctl(), &["--player", player, "position"]);
    if code == 0 {
        out.trim().parse::<f64>().unwrap_or(0.0)
    } else {
        0.0
    }
}

fn playerctl_length(player: &str) -> f64 {
    let (code, out, _) = crate::proc::run(
        &playerctl(),
        &["--player", player, "metadata", "--format", "{{mpris:length}}"],
    );
    if code == 0 {
        out.trim()
            .parse::<f64>()
            .map(|l| l / 1_000_000.0)
            .unwrap_or(0.0)
    } else {
        0.0
    }
}

fn get_media_info_for(player: Option<&str>) -> Option<MediaInfo> {
    let target_player = player?;
    let status_out = {
        let (code, out, _) = crate::proc::run(&playerctl(), &["--player", target_player, "status"]);
        if code == 0 { out.trim().to_string() } else { "Stopped".to_string() }
    };
    let player_name = playerctl_metadata(target_player, "{{playerName}}")
        .unwrap_or_else(|| target_player.to_string());
    let title = playerctl_metadata(target_player, "{{title}}").unwrap_or_default();
    let artist = playerctl_metadata(target_player, "{{artist}}").unwrap_or_default();
    let art_url = playerctl_metadata(target_player, "{{mpris:artUrl}}").unwrap_or_default();
    Some(MediaInfo {
        player: player_name,
        title,
        artist,
        art_url,
        status: status_out,
        position: playerctl_position(target_player),
        length: playerctl_length(target_player),
    })
}

#[allow(dead_code)]
fn run_playerctl(args: &[&str]) -> Option<String> {
    let (code, out, _) = crate::proc::run(&playerctl(), args);
    if code == 0 { Some(out.trim().to_string()) } else { None }
}

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
