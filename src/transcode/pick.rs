//! Picker via `pick`.

pub(crate) fn prompt_pick(title: &str, options: &[&str], on_select: &str) -> String {
    let entries = options.join("\n") + "\n";
    crate::pick::handle_with_stdin(
        &[
            "--title".into(),
            title.into(),
            "--on-select".into(),
            on_select.into(),
        ],
        &entries,
    )
}

#[cfg(test)]
pub(crate) fn prompt_pick_with(
    pick_bin: &str,
    title: &str,
    options: &[&str],
    on_select: &str,
) -> String {
    use crate::proc::run_with_stdin;
    let entries = options.join("\n") + "\n";
    let (code, _out, err) = run_with_stdin(
        pick_bin,
        &["--title", title, "--on-select", on_select],
        &entries,
    );
    if code != 0 {
        return format!("ERR (exit {}) {}", code, err.trim());
    }
    "OK".to_string()
}
