//! Menu: open/list/launch via quickshell IPC.

use crate::proc::{quickshell as qs, run};

fn usage() -> String {
    "usage: menu [apps | --only GROUP | --list [GROUP] | NAME]

commands:
  (none)            open the full menu
  apps              open the Applications group
  --only GROUP      open GROUP only
  --list [GROUP]    list entries (whole menu, or one GROUP)
  NAME              launch the app called NAME (quote names with spaces)

examples:
  menu
  menu apps
  menu --only Utilities
  menu --list
  menu --list Games
  menu 'System Monitor'"
        .to_string()
}

/// One quickshell IPC call against the `menu` target.
fn call(args: &[&str]) -> (i32, String, String) {
    let mut full: Vec<&str> = vec!["ipc", "call", "menu"];
    full.extend_from_slice(args);
    run(&qs(), &full)
}

/// A validated menu action, ready to execute against quickshell.
#[derive(Debug)]
enum Command {
    /// `menu` — open the full menu.
    Open,
    /// `menu apps` / `menu --only GROUP` — open one group.
    Only(String),
    /// `menu --list [GROUP]` — print entries (stdout, no trailing newline).
    List(Option<String>),
    /// `menu NAME` — launch an application by name.
    Launch(String),
}

/// Parse CLI args into a [`Command`]. Pure: unit-testable without IPC.
fn parse(args: &[String]) -> Result<Command, String> {
    fn unexpected(action: &str, extra: &[String]) -> String {
        format!(
            "ERR menu: '{}' takes no extra arguments (got: {})\n\n{}",
            action,
            extra.join(" "),
            usage()
        )
    }

    let (sub, rest) = match args.split_first() {
        None => return Ok(Command::Open),
        Some(pair) => pair,
    };
    match sub.as_str() {
        "apps" if rest.is_empty() => Ok(Command::Only("Applications".into())),
        "apps" => Err(unexpected("apps", rest)),
        "--only" => match rest {
            [] => Err(format!(
                "ERR menu: --only needs a group name\n\n{}",
                usage()
            )),
            [group] => Ok(Command::Only(group.clone())),
            _ => Err(format!(
                "ERR menu: --only takes exactly one GROUP\n\n{}",
                usage()
            )),
        },
        "--list" => match rest {
            [] => Ok(Command::List(None)),
            [group] => Ok(Command::List(Some(group.clone()))),
            _ => Err(format!(
                "ERR menu: --list takes at most one GROUP\n\n{}",
                usage()
            )),
        },
        other if other.starts_with('-') => Err(format!(
            "ERR menu: unknown option: {}\n\n{}",
            other,
            usage()
        )),
        name => {
            if !rest.is_empty() {
                // A spacey NAME arrives as one argv after quoting
                // (`menu 'My App'`); unquoted extra words mean the
                // user typed two names. Reject instead of silently
                // launching the first one.
                return Err(format!(
                    "ERR menu: NAME takes no extra arguments (got: {})\n\n{}",
                    rest.join(" "),
                    usage()
                ));
            }
            Ok(Command::Launch(name.to_owned()))
        }
    }
}

/// Format a transport-level IPC failure (non-zero exit). Empty
/// stderr still yields a readable message instead of a trailing
/// dangling space.
fn transport_err(code: i32, err: &str) -> String {
    match err.trim() {
        "" => format!("ERR (exit {code})"),
        err => format!("ERR (exit {code}) {err}"),
    }
}

/// Run an IPC subcommand that only ever succeeds silently and map
/// the result to the CLI convention: `OK` on exit 0, else the
/// transport error.
fn ok_or_err(args: &[&str]) -> String {
    let (code, _, err) = call(args);
    if code == 0 {
        "OK".to_string()
    } else {
        transport_err(code, &err)
    }
}

/// Interpret a `menu launch` reply. Quickshell prints `ok` on
/// success — and sometimes nothing at all; both mean success.
/// Anything else with a zero exit code is an application-level
/// error (the shell understood us and refused).
fn launch_reply(reply: &str) -> String {
    match reply.trim() {
        "" | "ok" => "OK".to_string(),
        other => format!("ERR {other}"),
    }
}

/// Execute a parsed command against quickshell.
fn execute(cmd: Command) -> String {
    match cmd {
        Command::Open => ok_or_err(&["open"]),
        Command::Only(group) => ok_or_err(&["only", &group]),
        Command::List(group) => {
            let (code, out, err) = call(&["list", group.as_deref().unwrap_or("")]);
            if code == 0 {
                out.trim_end().to_string()
            } else {
                transport_err(code, &err)
            }
        }
        Command::Launch(name) => {
            let (code, reply, err) = call(&["launch", &name]);
            if code != 0 {
                transport_err(code, &err)
            } else {
                launch_reply(&reply)
            }
        }
    }
}

pub fn handle(args: &[String]) -> String {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return usage();
    }
    match parse(args) {
        Ok(cmd) => execute(cmd),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn open_with_no_args() {
        assert!(matches!(parse(&[]), Ok(Command::Open)));
    }

    #[test]
    fn apps_maps_to_applications_group() {
        assert!(matches!(
            parse(&args("apps")),
            Ok(Command::Only(ref g)) if g == "Applications"
        ));
    }

    #[test]
    fn only_needs_one_group() {
        assert!(
            parse(&args("--only"))
                .unwrap_err()
                .starts_with("ERR menu: --only needs")
        );
        assert!(matches!(
            parse(&args("--only Utilities")),
            Ok(Command::Only(ref g)) if g == "Utilities"
        ));
        assert!(
            parse(&args("--only A B"))
                .unwrap_err()
                .starts_with("ERR menu: --only takes")
        );
    }

    #[test]
    fn list_takes_zero_or_one_group() {
        assert!(matches!(parse(&args("--list")), Ok(Command::List(None))));
        assert!(matches!(
            parse(&args("--list Games")),
            Ok(Command::List(Some(ref g))) if g == "Games"
        ));
        assert!(
            parse(&args("--list A B"))
                .unwrap_err()
                .starts_with("ERR menu: --list takes")
        );
    }

    #[test]
    fn name_with_spaces_is_one_name() {
        // `menu 'System Monitor'` arrives as a single argv.
        let argv = vec!["System Monitor".to_string()];
        assert!(matches!(
            parse(&argv),
            Ok(Command::Launch(ref n)) if n == "System Monitor"
        ));
    }

    #[test]
    fn unknown_dash_options_rejected() {
        assert!(
            parse(&args("--bogus"))
                .unwrap_err()
                .starts_with("ERR menu: unknown option:")
        );
        assert!(
            parse(&args("--"))
                .unwrap_err()
                .starts_with("ERR menu: unknown option:")
        );
    }

    #[test]
    fn extra_args_rejected() {
        assert!(
            parse(&args("apps Games"))
                .unwrap_err()
                .contains("takes no extra arguments")
        );
        let argv = vec!["Sys".to_string(), "extra".to_string()];
        assert!(
            parse(&argv)
                .unwrap_err()
                .contains("takes no extra arguments")
        );
    }

    #[test]
    fn transport_err_renders_exit_code() {
        assert_eq!(
            transport_err(1, "no such target"),
            "ERR (exit 1) no such target"
        );
        assert_eq!(transport_err(127, "  "), "ERR (exit 127)");
    }

    #[test]
    fn launch_reply_treats_empty_and_ok_as_success() {
        assert_eq!(launch_reply("ok"), "OK");
        assert_eq!(launch_reply("  ok\n"), "OK");
        assert_eq!(launch_reply(""), "OK", "shell printed nothing on success");
        assert_eq!(launch_reply("no such app: x"), "ERR no such app: x");
    }
}
