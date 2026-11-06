use crate::proc::{resolve_tool, run, shell_quote};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn search_paths() -> Vec<PathBuf> {
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    vec![PathBuf::from(format!("{}/Pictures", home)), PathBuf::from(format!("{}/Videos", home))]
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn media_type(path: &str) -> Option<&'static str> {
    let (code, out, _) = run("file", &["-b", "--mime-type", path]);
    if code != 0 { return None; }
    let mime = out.trim();
    if mime.starts_with("image/") { Some("picture") }
    else if mime.starts_with("video/") { Some("video") }
    else { None }
}

fn output_path(input: &str, format: &str, resolution: &str) -> String {
    let dir = std::path::Path::new(input).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
    let base = std::path::Path::new(input).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base);
    format!("{}/{}-{}.{}", dir, stem, resolution, format)
}

fn transcode_picture(input: &str, format: &str, resolution: &str, output: &str) -> bool {
    let resize = match resolution { "high" => "3160x>", "medium" => "2160x>", "low" => "1080x>", _ => return false };
    let magick = resolve_tool("magick");
    let status = match format {
        "jpg" => Command::new(&magick).args([input, "-resize", resize, "-quality", "85", "-strip", output]).status(),
        "png" => Command::new(&magick).args([input, "-resize", resize, "-strip", "-define", "png:compression-filter=5", "-define", "png:compression-level=9", "-define", "png:compression-strategy=1", "-define", "png:exclude-chunk=all", output]).status(),
        _ => return false,
    };
    status.map(|s| s.success()).unwrap_or(false)
}

fn transcode_video(input: &str, format: &str, resolution: &str, output: &str) -> bool {
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

pub fn handle(args: &[String]) -> String {
    let mut search_paths = search_paths();
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--path" {
            if i + 1 < args.len() { search_paths = vec![PathBuf::from(&args[i+1])]; i += 2; continue; }
            else { return "ERR --path needs a directory".to_string(); }
        }
        if args[i] == "--" { i += 1; while i < args.len() { positional.push(args[i].clone()); i += 1; } break; }
        if args[i].starts_with('-') { return format!("ERR unknown option: {}", args[i]); }
        positional.push(args[i].clone());
        i += 1;
    }
    let input = positional.first().cloned().unwrap_or_default();
    let format = positional.get(1).cloned().unwrap_or_default();
    let resolution = positional.get(2).cloned().unwrap_or_default();

    if input.is_empty() {
        let mut candidates = Vec::new();
        for dir in &search_paths {
            fn walk(dir: &PathBuf, depth: usize, out: &mut Vec<String>) {
                if depth == 0 { return; }
                if let Ok(entries) = fs::read_dir(dir) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.is_file() {
                            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
                            if matches!(ext.as_str(), "png"|"jpg"|"jpeg"|"webp"|"heic"|"tiff"|"bmp"|"mp4"|"mov"|"mkv"|"webm"|"avi") {
                                out.push(p.to_string_lossy().to_string());
                            }
                        } else if p.is_dir() && depth > 1 {
                            walk(&p, depth - 1, out);
                        }
                    }
                }
            }
            walk(dir, 4, &mut candidates);
        }
        candidates.sort();
        candidates.dedup();
        if candidates.is_empty() { return "ERR no pictures or videos found".to_string(); }
        let _ = run("pick", &["--title", "transcode", "--on-select", &format!("transcode {}", quote("{}"))]);
        return candidates.join("\n");
    }

    if !std::path::Path::new(&input).is_file() {
        return format!("ERR no such file: {}", input);
    }
    let mtype = match media_type(&input) {
        Some(t) => t,
        None => return format!("ERR not a picture or a video: {}", input),
    };

    let mut format = format;
    let mut resolution = resolution;

    if format.is_empty() {
        let (code, out, _) = run("pick", &["--title", "format", "--on-select", &format!("transcode {} {}", quote(&input), "{}")]);
        if code == 0 {
            let sel = out.trim().to_string();
            if !sel.is_empty() { format = sel; }
        }
        if format.is_empty() { return "OK".to_string(); }
    }

    if resolution.is_empty() {
        let (code, out, _) = run("pick", &["--title", "size", "--on-select", &format!("transcode {} {} {}", quote(&input), quote(&format), "{}")]);
        if code == 0 {
            let sel = out.trim().to_string();
            if !sel.is_empty() { resolution = sel; }
        }
        if resolution.is_empty() { return "OK".to_string(); }
    }

    let output = output_path(&input, &format, &resolution);

    let ok = if mtype == "picture" {
        transcode_picture(&input, &format, &resolution, &output)
    } else {
        transcode_video(&input, &format, &resolution, &output)
    };
    if !ok { return "ERR conversion failed".to_string(); }
    if !std::path::Path::new(&output).exists() { return "ERR conversion produced nothing".to_string(); }

    let uri = format!("file://{}", output);
    let cmd = format!("printf '%s\\n' {} | {} --type text/uri-list", shell_quote(&uri), wl_copy());
    let _ = run("sh", &["-c", &cmd]);
    format!("Saved {}", output)
}

fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_path_basic() {
        assert_eq!(output_path("/tmp/demo.mp4", "mp4", "1080p"), "/tmp/demo-1080p.mp4");
        assert_eq!(output_path("/tmp/demo.png", "jpg", "low"), "/tmp/demo-low.jpg");
    }
}
