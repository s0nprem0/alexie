//! Picker: feed candidates to `pick` via stdin, chain via
//! `--on-select` in a follow-up transcode process.

use crate::proc::run_with_stdin;

pub(crate) fn prompt_pick(title: &str, options: &[&str], on_select: &str) -> String {
    prompt_pick_with("pick", title, options, on_select)
}

/// Same as prompt_pick, with an explicit picker binary — the test
/// passes a fake absolute path instead of mutating PATH, which
/// races with other tests that also mutate PATH.
pub(crate) fn prompt_pick_with(pick_bin: &str, title: &str, options: &[&str], on_select: &str) -> String {
    let entries = options.join("\n") + "\n";
    let (code, _out, err) = run_with_stdin(pick_bin, &["--title", title, "--on-select", on_select], &entries);
    if code != 0 {
        return format!("ERR (exit {}) {}", code, err.trim());
    }
    "OK".to_string()
}

