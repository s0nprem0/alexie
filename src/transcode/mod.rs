//! Transcode pictures/videos: detect, prompt, convert.
//!
//! Split from a single 297-line transcode.rs:
//! - `pick` — stdin feeding, on-select chaining
//! - `convert` — media type, validation, magick/ffmpeg
//!
//! `handle()` below is just argument parsing/dispatch.

pub(crate) mod pick;
pub(crate) mod convert;

use convert::{format_size_options, media_type, output_path, transcode_picture, transcode_video, validate_format_resolution};
use pick::prompt_pick;
use std::env;
use std::fs;
use std::path::PathBuf;

fn search_paths() -> Vec<PathBuf> {
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    vec![PathBuf::from(format!("{}/Pictures", home)), PathBuf::from(format!("{}/Videos", home))]
}

fn quote(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }

fn wl_copy() -> String { env::var("WL_COPY").unwrap_or_else(|_| "wl-copy".to_string()) }

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



#[cfg(test)]
mod tests {
    use super::*;
    use super::convert::{format_size_options, media_type_from_mime, output_path, validate_format_resolution};
    use super::pick::prompt_pick_with;
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
