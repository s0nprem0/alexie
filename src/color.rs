#![allow(dead_code)]
use std::io::IsTerminal;

pub fn enabled() -> bool {
    // Unit tests assert on the rendered layout, not on terminal capabilities.
    // Without this, `cargo test` run from a terminal (or a builder that hands
    // the process a tty) emits SGR codes and the table tests fail.
    if cfg!(test) {
        return false;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("ALEXIE_COLOR").as_deref() == Ok("0") {
        return false;
    }
    std::io::stdout().is_terminal()
}

pub fn wrap(text: &str, code: &str) -> String {
    if enabled() {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}

pub fn bold(text: &str) -> String { wrap(text, "1") }
pub fn dim(text: &str) -> String { wrap(text, "2") }
pub fn red(text: &str) -> String { wrap(text, "31") }
pub fn green(text: &str) -> String { wrap(text, "32") }
pub fn yellow(text: &str) -> String { wrap(text, "33") }
pub fn blue(text: &str) -> String { wrap(text, "34") }
pub fn magenta(text: &str) -> String { wrap(text, "35") }
pub fn cyan(text: &str) -> String { wrap(text, "36") }
