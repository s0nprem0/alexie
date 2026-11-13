// Help text and the command table, split from main.rs.

pub(crate) fn is_help_flag(s: &str) -> bool {
    matches!(s, "--help" | "-h" | "help")
}

pub(crate) fn help_text(cmd: Option<&str>) -> String {
    match cmd {
        None => {
            let mut s = String::new();
            s.push_str("usage: alexie <command...> | alexie daemon\n\n");
            s.push_str("commands:\n");
            for (name, desc, _) in COMMANDS {
                s.push_str(&format!("  {:<15} {}\n", name, desc));
            }
            s.push_str("\nhelp: alexie --help | alexie <command> --help | alexie help <command>\n");
            s
        }
        Some(c) => {
            if let Some((_, desc, subs)) = COMMANDS.iter().find(|(n, _, _)| *n == c) {
                let mut s = String::new();
                s.push_str(&format!("usage: alexie {} <subcommand>...\n\n", c));
                s.push_str(&format!("{}\n\n", desc));
                s.push_str("subcommands:\n");
                s.push_str(subs);
                if !subs.ends_with('\n') {
                    s.push('\n');
                }
                s
            } else {
                format!("unknown command: {}\n\n{}", c, help_text(None))
            }
        }
    }
}

pub(crate) const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "monitors",
        "display scaling, mode, position, mirroring, workspaces.",
        "  status, list, set OUTPUT KEY VALUE\n  scale set|up|down|reset\n  brightness up|down|max|min|set N\n  identify, wake, recover\n  internal on|off|toggle\n  mirror on|off|toggle\n  lid close|open|toggle|reconcile\n  try OUTPUT KEY VALUE, keep, revert\n  workspaces sync|show|reset\n  workspace N, workspace-step next|prev\n  move-window N, identities, sync-identities\n",
    ),
    (
        "theme",
        "palette listing and switching.",
        "  list [--json], current, next, prev, state, migrate\n  set NAME, info [NAME], export, doctor, apply, auto\n  favorite add|remove|toggle NAME, pin, unpin\n  wallpaper [FILE] [auto|light|dark], wallpaper-mode, cursor-size N\n  switch WALLPAPER [auto|light|dark], presets, vars, render\n",
    ),
    (
        "wallpaper",
        "wallpaper set/watch and library.",
        "  current, status, set FILE, list, library\n  next, prev, rotate, pause, resume\n  watch, watch-foreground\n",
    ),
    (
        "power",
        "power profiles and charge limits.",
        "  show|status, apply, set ac|battery PROFILE\n  charge full|limit N\n  watch\n",
    ),
    (
        "screentime",
        "screen time tracking.",
        "  show, json, reset, path, db-path, panel, watch\n",
    ),
    (
        "status",
        "system status (network/sysmon/audio/bluetooth).",
        "  network [--json] [--rescan]\n  sysmon [--json]\n  audio [--json]\n  bluetooth [--json] [--scan]\n  list\n",
    ),
    (
        "network",
        "wifi + IP + VPN status.",
        "  [--json] [--rescan]\n",
    ),
    ("sysmon", "CPU/GPU/RAM/disk monitor.", "  [--json]\n"),
    ("audio", "PipeWire sinks/sources/media.", "  [--json]\n"),
    ("bluetooth", "bluetooth devices.", "  [--json] [--scan]\n"),
    ("focus", "focus mode.", "  start, stop, extend, status\n"),
    (
        "snapshot",
        "btrfs snapshots.",
        "  create, list, restore, prune\n",
    ),
    ("taildrop", "receive taildrop files.", "  (runs until killed)\n"),
    ("webapp", "websites as apps.", "  add, list, remove\n"),
    (
        "shot",
        "screenshots.",
        "  smart, region, windows, fullscreen, window\n  copy, save, preview, annotate, redact, text\n  db list|status|add|remove|clear|check [--json]\n  db color|ocr add|list|remove|clear, path, thumbs\n",
    ),
    (
        "record",
        "screen recording.",
        "  --status, --stop, --fullscreen, --audio\n",
    ),
    (
        "transcode",
        "picture/video resize.",
        "  picture|video ...\n",
    ),
    (
        "webcam-overlay",
        "webcam overlay.",
        "  start, stop, resize, list\n",
    ),
    (
        "clipboard",
        "clipboard history.",
        "  --copy, --list, --text, --image\n",
    ),
    (
        "emoji",
        "emoji picker (copy + type).",
        "  (no subcommands)\n",
    ),
    ("performance", "performance profile extras.", "  status, apply, watch\n"),
    (
        "pick",
        "generic picker.",
        "  --title, --placeholder, --input\n",
    ),
    ("reminder", "reminders.", "  show, count, clear\n"),
    (
        "share",
        "file sharing.",
        "  uri, path, edit, folder, compress\n",
    ),
    (
        "share-picker",
        "portal picker config.",
        "  status [--json], apply, check\n",
    ),
    ("shortcuts", "keyboard shortcuts.", "  (no subcommands)\n"),
    (
        "toggle",
        "nightlight, idle, dnd, stay-awake.",
        "  nightlight, idle, dnd, stayawake\n  status, list\n",
    ),
    (
        "menu",
        "application and action menu.",
        "  apps, --list, --only\n",
    ),
    (
        "apps",
        "app launcher backend.",
        "  list [--json] [--search TEXT]\n  launch NAME, bookmark add|remove|list|clear\n  history web|file clear|list, web search|open\n  files index|search|open|history\n",
    ),
    ("capture", "capture text/qr/share.", "  text, qr, share\n"),
    ("daemon", "run the alexie daemon.", "  (no subcommands)\n"),
    ("ping", "health check.", "  (no subcommands)\n"),
];
