//! Media detection and conversion: file(1) type, validate,
//! magick for pictures, ffmpeg for video.

use crate::proc::{resolve_tool, run};
use std::process::Command;

pub(crate) fn media_type(path: &str) -> Option<&'static str> {
    let (code, out, _) = run("file", &["-b", "--mime-type", path]);
    if code != 0 { return None; }
    media_type_from_mime(out.trim())
}

pub(crate) fn media_type_from_mime(mime: &str) -> Option<&'static str> {
    if mime.starts_with("image/") { Some("picture") }
    else if mime.starts_with("video/") { Some("video") }
    else { None }
}

/// Validate format/resolution for a media type, mirroring legacy
/// transcode_picture/transcode_video `die` messages. Pure, so it
/// can be tested without faking a `file` binary on PATH.
pub(crate) fn validate_format_resolution(mtype: &str, format: &str, resolution: &str) -> Result<(), String> {
    let (formats, sizes) = format_size_options(mtype);
    if !formats.contains(&format) {
        return Err(format!(
            "ERR unknown {} format: {} (try: {})",
            mtype,
            format,
            formats.join(", ")
        ));
    }
    if !sizes.contains(&resolution) {
        return Err(format!(
            "ERR unknown {} size: {} (try: {})",
            mtype,
            resolution,
            sizes.join(", ")
        ));
    }
    Ok(())
}

pub(crate) fn output_path(input: &str, format: &str, resolution: &str) -> String {
    let dir = std::path::Path::new(input).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
    let base = std::path::Path::new(input).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base);
    format!("{}/{}-{}.{}", dir, stem, resolution, format)
}

/// Formats/sizes offered in the picker, mirroring legacy transcode.sh.
pub(crate) fn format_size_options(mtype: &str) -> (&'static [&'static str], &'static [&'static str]) {
    if mtype == "picture" {
        (&["jpg", "png"], &["high", "medium", "low"])
    } else {
        (&["mp4", "gif"], &["4k", "1080p", "720p"])
    }
}

/// Hand a list to the picker and return its result.
///
/// Legacy flow: pick reads stdin, exits immediately, and the shell
/// later runs `--on-select` in a *new* transcode process which
/// prompts for the next answer. There is no synchronous selection
/// to read back — the old code waited on pick's stdout (always
/// empty) and returned "OK" without ever converting, and it never
/// fed the candidates to pick at all.
pub(crate) fn transcode_picture(input: &str, format: &str, resolution: &str, output: &str) -> bool {
    let resize = match resolution { "high" => "3160x>", "medium" => "2160x>", "low" => "1080x>", _ => return false };
    let magick = resolve_tool("magick");
    let status = match format {
        "jpg" => Command::new(&magick).args([input, "-resize", resize, "-quality", "85", "-strip", output]).status(),
        "png" => Command::new(&magick).args([input, "-resize", resize, "-strip", "-define", "png:compression-filter=5", "-define", "png:compression-level=9", "-define", "png:compression-strategy=1", "-define", "png:exclude-chunk=all", output]).status(),
        _ => return false,
    };
    status.map(|s| s.success()).unwrap_or(false)
}

pub(crate) fn transcode_video(input: &str, format: &str, resolution: &str, output: &str) -> bool {
    let scale = match resolution { "4k" => "scale=-2:2160", "1080p" => "scale=-2:1080", "720p" => "scale=-2:720", _ => return false };
    let ffmpeg = resolve_tool("ffmpeg");
    let status = match format {
        "mp4" => {
            if resolution == "4k" {
                Command::new(&ffmpeg).args(["-y", "-i", input, "-vf", scale, "-c:v", "libx265", "-preset", "slow", "-crf", "24", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", output]).status()
            } else {
                Command::new(&ffmpeg).args(["-y", "-i", input, "-vf", scale, "-c:v", "libx264", "-preset", "fast", "-crf", "23", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", output]).status()
            }
        }
        "gif" => {
            let vf = format!("fps=10,{}:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse", scale);
            Command::new(&ffmpeg).args(["-y", "-i", input, "-vf", &vf, output]).status()
        }
        _ => return false,
    };
    status.map(|s| s.success()).unwrap_or(false)
}

