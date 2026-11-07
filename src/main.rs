//! alexie entry point.
//!
//! Split for readability:
//! - `help` — command table and help text
//! - `dispatch` — command dispatcher (handle_args/handle_command)
//! - `daemon` — daemon socket, JSON protocol, threaded clients
//! - `client` — CLI client (daemon or local fallback, stdin forwarding)
//!
//! `main()` below is just argument routing between those.

mod client;
mod daemon;
mod dbus;
mod apps;
mod capture;
mod clipboard;
mod color;
mod config;
mod dispatch;
mod emoji;
mod focus;
mod help;
mod pick;
mod reminder;
mod share;
mod share_picker;
mod shortcuts;
mod snapshot;
mod table;
mod webapp;
mod menu;
mod webcam_overlay;
mod transcode;
mod record;
mod proc;
mod monitors;
mod power;
mod power_watch;
mod screentime;
mod shot;
mod status;
mod theme;
mod toggle;
mod wallpaper;

// Back-compat for dbus.rs (`crate::handle_command`).
pub use dispatch::handle_command;

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 && args[1] == "help" {
        print!("{}", help::help_text(Some(&args[2])));
        std::process::exit(0);
    }
    if args.len() < 2 || help::is_help_flag(&args[1]) || args[1] == "--help" || args[1] == "-h" {
        print!("{}", help::help_text(None));
        std::process::exit(0);
    }
    if args.len() >= 3 && help::is_help_flag(&args[2]) {
        print!("{}", help::help_text(Some(&args[1])));
        std::process::exit(0);
    }

    // power watch: run the watcher (blocks).
    if args.len() >= 3 && (args[1] == "power" || args[1] == "powerprofile") && args[2] == "watch" {
        power_watch::run();
        return;
    }
    // screentime watch: run the tracker (blocks).
    if args.len() >= 3 && args[1] == "screentime" && args[2] == "watch" {
        screentime::watch();
        return;
    }
    // clipboard watch/store: run locally, never via the daemon
    // socket. Watch blocks, and store is the wl-paste child mode
    // that must work even with the daemon down.
    if args.len() >= 3 && args[1] == "clipboard" && args[2] == "watch" {
        clipboard::watch();
        return;
    }
    if args.len() >= 3 && args[1] == "clipboard" && args[2] == "store" {
        clipboard::store();
        return;
    }
    if args.len() < 2 {
        eprintln!("usage: alexie <command...> | alexie daemon");
        std::process::exit(2);
    }
    if args[1] == "daemon" {
        daemon::run_daemon();
    } else {
        client::run_client(&args[1..]);
    }
}
