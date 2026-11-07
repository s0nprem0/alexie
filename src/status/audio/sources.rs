//! Media sources via playerctl.

use super::super::common::playerctl;
use super::types::{load_current_player, MediaInfo, MediaSource};
pub(crate) fn get_all_media_sources() -> Vec<MediaSource> {
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

pub(crate) fn playerctl_metadata(player: &str, format: &str) -> Option<String> {
    let (code, out, _) = crate::proc::run(&playerctl(), &["--player", player, "metadata", "--format", format]);
    if code == 0 {
        Some(out.trim().to_string())
    } else {
        None
    }
}

pub(crate) fn playerctl_position(player: &str) -> f64 {
    let (code, out, _) = crate::proc::run(&playerctl(), &["--player", player, "position"]);
    if code == 0 {
        out.trim().parse::<f64>().unwrap_or(0.0)
    } else {
        0.0
    }
}

pub(crate) fn playerctl_length(player: &str) -> f64 {
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

pub(crate) fn get_media_info_for(player: Option<&str>) -> Option<MediaInfo> {
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
pub(crate) fn run_playerctl(args: &[&str]) -> Option<String> {
    let (code, out, _) = crate::proc::run(&playerctl(), args);
    if code == 0 { Some(out.trim().to_string()) } else { None }
}
