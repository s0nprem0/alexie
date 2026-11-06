use std::process::Command;

const GROUPS: &str = r#"
theme|theme||list current set wallpaper wallpaper-mode state migrate auto favorite pin unpin export doctor cursor-size next prev apply info|palettes: list, set, wallpaper FILE, state, migrate, auto, favorite, pin, export, info, doctor, cursor-size
appearance|appearance||toggle open close state|appearance panel
textsize|textsize|text|up down show set reset apply|global text size: up, down, set N, reset
wallpaper|wallpaper||list library set add next prev rotate rotation generated pause resume status apply current|desktop background: set FILE, library, next, rotate, current
display|monitors|monitors|scale internal lid mirror brightness recover wake workspace workspace-step move-window workspaces list set try keep revert identify status|monitors: scale, internal, mirror, status
power|powerprofile|powerprofile|set apply show status automation set-automation charge|power profiles per source and charge ceiling: set ac|battery PROFILE, charge full|limit N
wifi|wifi||band dns password qr|Wi-Fi: band, DNS, password, QR
audio|audio-output|audio-output|set switch|audio output: set, switch
shot|shot|screenshot|smart region windows fullscreen window scroll copy save annotate preview redact text cancel copy-path copy-uri edit-last|screenshots: smart, region, windows, fullscreen
record|record||--status --stop --fullscreen --audio --webcam-device --webcam-size|screen recording
clipboard|clipboard||--copy|clipboard history popup
emoji|emoji|||emoji picker
menu|menu||apps --list --only|application and action menu
webapp|webapp||add list remove|websites as applications: add, list, remove
pick|pick||--title --placeholder --input --allow-empty --grouped --on-select --on-cancel|the generic picker
reminder|reminder||show count clear -i|set and list reminders
toggle|toggle||nightlight idle dnd stayawake status list|night light, idle, do-not-disturb
snapshot|snapshot||create list restore prune|btrfs snapshots of /home: create, list, restore, prune
update|rebuild|rebuild|switch boot test|rebuild and switch the system
bar|bar||toggle on off reset reload move enable disable background position get set modules manage tray|the status bar: toggle, on off, layout, settings
focus|focus||start stop extend status|focus mode: start, stop, extend, status
screentime|screentime||show json reset path panel|screen time for today: show, json, reset
"#;

#[derive(Clone)]
pub struct Group {
    pub name: String,
    pub cmd: String,
    pub aliases: Vec<String>,
    pub subs: Vec<String>,
    pub desc: String,
}

pub fn parse_groups() -> Vec<Group> {
    let mut out = Vec::new();
    for line in GROUPS.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 5 { continue; }
        let name = parts[0].to_string();
        let cmd = parts[1].to_string();
        let aliases: Vec<String> = parts[2].split_whitespace().map(|s| s.to_string()).collect();
        let subs: Vec<String> = parts[3].split_whitespace().map(|s| s.to_string()).collect();
        let desc = parts[4..].join("|");
        out.push(Group { name, cmd, aliases, subs, desc });
    }
    out
}

pub fn available(cmd: &str) -> bool {
    let first = cmd.split_whitespace().next().unwrap_or("");
    Command::new("sh").args(["-c", &format!("command -v {} >/dev/null 2>&1", first)]).status().map(|s| s.success()).unwrap_or(false)
}

pub fn find_group<'a>(groups: &'a [Group], name: &str) -> Option<&'a Group> {
    groups.iter().find(|g| g.name == name || g.aliases.iter().any(|a| a == name))
}

pub fn resolve(groups: &[Group], name: &str) -> Option<String> {
    find_group(groups, name).map(|g| g.cmd.clone())
}

pub fn usage(groups: &[Group]) {
    println!("alexie — desktop command center\n");
    println!("Usage:");
    println!("  alexie <group> [args...]");
    println!("  alexie help <group>");
    println!("  alexie commands [--aliases]");
    println!("  alexie resolve <group>");
    println!("  alexie subcommands <group>\n");
    println!("Groups:");
    let mut rows: Vec<Vec<String>> = Vec::new();
    for g in groups {
        if !available(&g.cmd) { continue; }
        rows.push(vec![g.name.clone(), g.desc.clone(), g.aliases.join(", ")]);
    }
    let mut widths = [0usize; 2];
    for r in &rows {
        for i in 0..2 {
            widths[i] = widths[i].max(r[i].chars().count());
        }
    }
    for r in &rows {
        if r[2].is_empty() {
            println!("  {:<w0$}  {}", r[0], r[1], w0 = widths[0]);
        } else {
            println!("  {:<w0$}  {:<w1$}  (also: {})", r[0], r[1], r[2], w0 = widths[0], w1 = widths[1]);
        }
    }
}

pub fn group_names(groups: &[Group], aliases: bool) {
    for g in groups {
        if !available(&g.cmd) { continue; }
        println!("{}", g.name);
        if aliases {
            for a in &g.aliases { println!("{}", a); }
        }
    }
}

pub fn subcommands(groups: &[Group], name: &str) -> Result<(), i32> {
    let g = find_group(groups, name).ok_or(2)?;
    if !available(&g.cmd) { return Err(2); }
    for s in &g.subs { println!("{}", s); }
    Ok(())
}

/// Handle anixie-style meta commands. Returns Some(exit_code) if handled.
pub fn try_meta(args: &[String], groups: &[Group]) -> Option<i32> {
    match args.get(1).map(|s| s.as_str()) {
        None | Some("") | Some("-h") | Some("--help") => {
            if args.len() > 2 {
                eprintln!("alexie: unexpected argument \"{}\"", args[2]);
                return Some(2);
            }
            usage(groups);
            Some(0)
        }
        Some("help") => {
            if args.len() == 2 {
                usage(groups);
                Some(0)
            } else if args.len() == 3 {
                let name = &args[2];
                if name.starts_with('-') {
                    eprintln!("alexie: unknown option \"{}\"", name);
                    return Some(2);
                }
                let cmd = match resolve(groups, name) {
                    Some(c) => c,
                    None => { eprintln!("alexie: no group called \"{}\"", name); return Some(2); }
                };
                let first = cmd.split_whitespace().next().unwrap_or("");
                if !available(&cmd) { eprintln!("alexie: {} is not available on this host", first); return Some(127); }
                // Print subcommands instead of execing.
                if let Some(g) = find_group(groups, name) {
                    for s in &g.subs { println!("{}", s); }
                }
                Some(0)
            } else {
                eprintln!("alexie: help takes at most one group");
                Some(2)
            }
        }
        Some("commands") => {
            if args.len() == 2 {
                group_names(groups, false);
                Some(0)
            } else if args.len() == 3 && args[2] == "--aliases" {
                group_names(groups, true);
                Some(0)
            } else {
                eprintln!("alexie: commands takes at most one option");
                Some(2)
            }
        }
        Some("resolve") => {
            if args.len() != 3 {
                eprintln!("alexie: resolve needs exactly one group (try: anixie commands)");
                return Some(2);
            }
            match resolve(groups, &args[2]) {
                Some(c) => { println!("{}", c); Some(0)}
                None => { eprintln!("alexie: no group called \"{}\"", args[2]); Some(2)}
            }
        }
        Some("subcommands") => {
            if args.len() != 3 {
                eprintln!("alexie: subcommands needs exactly one group (try: anixie commands)");
                return Some(2);
            }
            match subcommands(groups, &args[2]) {
                Ok(()) => Some(0),
                Err(_) => { eprintln!("alexie: no group called \"{}\"", args[2]); Some(2)}
            }
        }
        _ => None,
    }
}
