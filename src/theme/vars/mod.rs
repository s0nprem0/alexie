//! Colour vars: build/cache, `vars`/`render` commands, presets.
//!
//! Split from a single 349-line vars.rs:
//! - `build` — hex maths, palette->vars, cache, generation
//! - `commands` — parse args, theme_vars/theme_render, presets
//!
//! Re-exported here so `vars::*` callers are unchanged.

mod build;
mod commands;

pub(crate) use commands::{presets, theme_render, theme_vars};
