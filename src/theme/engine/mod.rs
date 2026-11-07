//! Native theming engine: materialise, generate, set, apply, auto, doctor.
//!
//! Split from a single 1300-line engine.rs for readability:
//! - `color` — WCAG colour maths, palette validation/fix, accent name
//! - `generate` — matugen palette transform, wallpaper palette generation
//! - `materialise` — generation dir, kitty conf, live symlink, state lock
//! - `apps` — tell_apps, hooks, set/apply
//! - `auto` — light/dark schedule, pair finding
//! - `doctor` — live-state and generated-file checks
//!
//! Ports the legacy theme.sh + lib/engine/{core,materialize,apps,generate}.sh
//! logic to Rust. The parts that remain external are the same tools
//! legacy shells out to, now via explicit env-overridable names so
//! tests can fake them on PATH:
//! - `theme-apps` renders the application templates (like @THEME_APPS@)
//! - `matugen`/`magick`/`ffmpeg` for wallpaper palette generation
//! - quickshell/hyprctl/gsettings/tmux for tell_apps, plus the
//!   native wallpaper apply (all best-effort, like legacy `|| true`)
//!
//! Palettes come from the embedded themes.json (identical copy of
//! anixie's themes.json, verified by `diff` in development), not an
//! external @PALETTES@ path, so no Nix placeholder plumbing is needed
//! for the curated path. The renderer record written into a
//! generation is therefore "embedded themes.json <theme-apps path>"
//! rather than the legacy Nix store paths; `apply` treats an
//! existing renderer file as "made by a compatible renderer".

pub mod color;
pub mod generate;
pub mod materialise;
pub mod render_apps;
pub mod apps;
pub mod auto;
pub mod doctor;

// Re-export the dispatcher-facing entry points, so
// `crate::theme::engine::set_theme` etc. keep working.
pub(crate) use apps::{apply_theme, set_theme};
pub(crate) use auto::apply_auto;
pub(crate) use doctor::doctor;
pub(crate) use generate::wallpaper_theme;
