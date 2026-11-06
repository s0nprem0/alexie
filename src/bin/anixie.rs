use std::env;
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
struct Group {
    name: String,
    cmd: String,
    aliases: Vec<String>,
    subs: Vec<String>,
    desc: String,
}

fn parse_groups() -> Vec<Group> {
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

fn available(cmd: &str) -> bool {
    let first = cmd.split_whitespace().next().unwrap_or("");
    Command::new("sh").args(["-c", &format!("command -v {} >/dev/null 2>&1", first)]).status().map(|s| s.success()).unwrap_or(false)
}

fn find_group<'a>(groups: &'a [Group], name: &str) -> Option<&'a Group> {
    groups.iter().find(|g| g.name == name || g.aliases.iter().any(|a| a == name))
}

fn resolve(groups: &[Group], name: &str) -> Option<String> {
    find_group(groups, name).map(|g| g.cmd.clone())
}

fn usage(groups: &[Group]) {
    println!("anixie — desktop command center\n");
    println!("Usage:");
    println!("  anixie <group> [args...]");
    println!("  anixie help <group>");
    println!("  anixie commands [--aliases]");
    println!("  anixie resolve <group>");
    println!("  anixie subcommands <group>\n");
    println!("Groups:");
    for g in groups {
        if !available(&g.cmd) { continue; }
        if !g.aliases.is_empty() {
            println!("  {:<11} {} (also: {})", g.name, g.desc, g.aliases.join(", "));
        } else {
            println!("  {:<11} {}", g.name, g.desc);
        }
    }
}

fn group_names(groups: &[Group], aliases: bool) {
    for g in groups {
        if !available(&g.cmd) { continue; }
        println!("{}", g.name);
        if aliases {
            for a in &g.aliases { println!("{}", a); }
        }
    }
}

fn subcommands(groups: &[Group], name: &str) -> Result<(), i32> {
    let g = find_group(groups, name).ok_or(2)?;
    if !available(&g.cmd) { return Err(2); }
    for s in &g.subs { println!("{}", s); }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let groups = parse_groups();

    match args.get(1).map(|s| s.as_str()) {
        None | Some("") | Some("-h") | Some("--help") => {
            if args.len() > 2 {
                eprintln!("anixie: unexpected argument \"{}\"", args[2]);
                std::process::exit(2);
            }
            usage(&groups);
            std::process::exit(0);
        }
        Some("help") => {
            if args.len() == 2 {
                usage(&groups);
                std::process::exit(0);
            } else if args.len() == 3 {
                let name = &args[2];
                if name.starts_with('-') {
                    eprintln!("anixie: unknown option \"{}\"", name);
                    std::process::exit(2);
                }
                let cmd = match resolve(&groups, name) {
                    Some(c) => c,
                    None => { eprintln!("anixie: no group called \"{}\"", name); std::process::exit(2); }
                };
                let first = cmd.split_whitespace().next().unwrap_or("");
                if !available(&cmd) { eprintln!("anixie: {} is not available on this host", first); std::process::exit(127); }
                let mut c = Command::new(first);
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if parts.len() > 1 { c.args(&parts[1..]); }
                c.arg("--help");
                let status = c.status().unwrap_or_else(|_| std::process::exit(1));
                std::process::exit(status.code().unwrap_or(1));
            } else {
                eprintln!("anixie: help takes at most one group");
                std::process::exit(2);
            }
        }
        Some("commands") => {
            if args.len() == 2 {
                group_names(&groups, false);
                std::process::exit(0);
            } else if args.len() == 3 && args[2] == "--aliases" {
                group_names(&groups, true);
                std::process::exit(0);
            } else {
                eprintln!("anixie: commands takes at most one option");
                std::process::exit(2);
            }
        }
        Some("resolve") => {
            if args.len() != 3 {
                eprintln!("anixie: resolve needs exactly one group (try: anixie commands)");
                std::process::exit(2);
            }
            match resolve(&groups, &args[2]) {
                Some(c) => { println!("{}", c); std::process::exit(0); }
                None => { eprintln!("anixie: no group called \"{}\"", args[2]); std::process::exit(2); }
            }
        }
        Some("subcommands") => {
            if args.len() != 3 {
                eprintln!("anixie: subcommands needs exactly one group (try: anixie commands)");
                std::process::exit(2);
            }
            match subcommands(&groups, &args[2]) {
                Ok(()) => std::process::exit(0),
                Err(_) => { eprintln!("anixie: no group called \"{}\"", args[2]); std::process::exit(2); }
            }
        }
        Some(opt) if opt.starts_with('-') => {
            eprintln!("anixie: unknown option \"{}\"", opt);
            std::process::exit(2);
        }
        Some(_) => {}
    }

    let group = &args[1];
    let cmd = match resolve(&groups, group) {
        Some(c) => c,
        None => { eprintln!("anixie: no group called \"{}\" (try: anixie commands)", group); std::process::exit(2); }
    };
    let first = cmd.split_whitespace().next().unwrap_or("");
    if !available(&cmd) {
        eprintln!("anixie: {} is not available on this host", first);
        std::process::exit(127);
    }

    let mut exec_args: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
    for a in args.iter().skip(2) { exec_args.push(a.clone()); }

    let status = Command::new(&exec_args[0])
        .args(&exec_args[1..])
        .status()
        .unwrap_or_else(|_| std::process::exit(1));
    std::process::exit(status.code().unwrap_or(1));
}