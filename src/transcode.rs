use crate::proc::{resolve_tool, run, run_with_stdin};
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
    media_type_from_mime(out.trim())
}

fn media_type_from_mime(mime: &str) -> Option<&'static str> {
    if mime.starts_with("image/") { Some("picture") }
    else if mime.starts_with("video/") { Some("video") }
    else { None }
}

/// Validate format/resolution for a media type, mirroring legacy
/// transcode_picture/transcode_video `die` messages. Pure, so it
/// can be tested without faking a `file` binary on PATH.
fn validate_format_resolution(mtype: &str, format: &str, resolution: &str) -> Result<(), String> {
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

fn output_path(input: &str, format: &str, resolution: &str) -> String {
    let dir = std::path::Path::new(input).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
    let base = std::path::Path::new(input).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base);
    format!("{}/{}-{}.{}", dir, stem, resolution, format)
}

/// Formats/sizes offered in the picker, mirroring legacy transcode.sh.
fn format_size_options(mtype: &str) -> (&'static [&'static str], &'static [&'static str]) {
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
fn prompt_pick(title: &str, options: &[&str], on_select: &str) -> String {
    prompt_pick_with("pick", title, options, on_select)
}

/// Same as prompt_pick, with an explicit picker binary — the test
/// passes a fake absolute path instead of mutating PATH, which
/// races with other tests that also mutate PATH.
fn prompt_pick_with(pick_bin: &str, title: &str, options: &[&str], on_select: &str) -> String {
    let entries = options.join("\n") + "\n";
    let (code, _out, err) = run_with_stdin(pick_bin, &["--title", title, "--on-select", on_select], &entries);
    if code != 0 {
        return format!("ERR (exit {}) {}", code, err.trim());
    }
    "OK".to_string()
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
        return prompt_pick("transcode", &candidates.iter().map(|s| s.as_str()).collect::<Vec<_>>(), &format!("transcode {}", quote("{}")));
    }

    if !std::path::Path::new(&input).is_file() {
        return format!("ERR no such file: {}", input);
    }
    let mtype = match media_type(&input) {
        Some(t) => t,
        None => return format!("ERR not a picture or a video: {}", input),
    };

    let (formats, sizes) = format_size_options(mtype);

    if format.is_empty() {
        return prompt_pick(
            "format",
            formats,
            &format!("transcode {} {{}}", quote(&input)),
        );
    }

    if resolution.is_empty() {
        // Validate format before offering sizes, like legacy's
        // transcode_picture/transcode_video die on unknown format.
        if !formats.contains(&format.as_str()) {
            return format!("ERR unknown {} format: {} (try: {})", mtype, format, formats.join(", "));
        }
        return prompt_pick(
            "size",
            sizes,
            &format!("transcode {} {} {{}}", quote(&input), quote(&format)),
        );
    }

    if let Err(e) = validate_format_resolution(mtype, &format, &resolution) {
        return e;
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
    let _ = crate::proc::copy_text_to_clipboard(&wl_copy(), "text/uri-list", &format!("{}\n", uri), &[]);
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

    #[test]
    fn format_size_options_match_legacy() {
        assert_eq!(format_size_options("picture").0, ["jpg", "png"]);
        assert_eq!(format_size_options("picture").1, ["high", "medium", "low"]);
        assert_eq!(format_size_options("video").0, ["mp4", "gif"]);
        assert_eq!(format_size_options("video").1, ["4k", "1080p", "720p"]);
    }

    #[test]
    fn prompt_pick_feeds_stdin_and_returns_ok() {
        // Regression: the old code ran pick without stdin and waited
        // for a selection on stdout, so it never converted anything.
        // With a fake `pick` binary (absolute path, no PATH mutation
        // which would race other tests) that records its stdin, the
        // entries must arrive and the result must be OK (the shell
        // runs --on-select later, in a new process).
        let dir = std::env::temp_dir().join(format!(
            "alexie-transcode-pick-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("pick-stdin.log");
        let pick = dir.join("pick");
        std::fs::write(
            &pick,
            format!("#!/bin/sh\ncat > {}\n", log.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&pick, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let out = prompt_pick_with(
            &pick.to_string_lossy(),
            "format",
            &["jpg", "png"],
            "transcode '/tmp/x.png' {}",
        );
        assert_eq!(out, "OK", "got: {out}");
        let fed = std::fs::read_to_string(&log).unwrap();
        assert_eq!(fed, "jpg\npng\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transcode_unknown_format_is_usage() {
        // Pure validation, no PATH faking needed.
        assert_eq!(
            validate_format_resolution("picture", "bogusfmt", "low"),
            Err("ERR unknown picture format: bogusfmt (try: jpg, png)".to_string())
        );
        assert_eq!(
            validate_format_resolution("video", "mp4", "bogus"),
            Err("ERR unknown video size: bogus (try: 4k, 1080p, 720p)".to_string())
        );
        assert!(validate_format_resolution("picture", "jpg", "low").is_ok());
        assert_eq!(media_type_from_mime("image/png"), Some("picture"));
        assert_eq!(media_type_from_mime("video/mp4"), Some("video"));
        assert_eq!(media_type_from_mime("application/octet-stream"), None);
    }
}
