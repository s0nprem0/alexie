use std::env;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn dest() -> String { env::var("DEST").unwrap_or_else(|_| "/.snapshots/home".to_string()) }
fn keep_default() -> i64 { env::var("KEEP").ok().and_then(|s| s.parse().ok()).unwrap_or(5) }
fn btrfs() -> String { env::var("BTRFS").unwrap_or_else(|_| "btrfs".to_string()) }
fn rsync() -> String { env::var("RSYNC").unwrap_or_else(|_| "rsync".to_string()) }

// avoid libc dependency: check via id
fn is_root() -> bool {
    let (code, out, _) = run("id", &["-u"]);
    code == 0 && out.trim() == "0"
}

fn run_root(cmd: &str, args: &[&str]) -> (i32, String, String) {
    if is_root() {
        run(cmd, args)
    } else {
        let mut all: Vec<&str> = vec![cmd];
        all.extend_from_slice(args);
        run("sudo", &all)
    }
}

pub fn handle(args: &[String]) -> String {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "" | "create" => {
            let label = if sub == "create" { args.get(1).cloned().unwrap_or_else(|| "manual".to_string()) } else { "manual".to_string() };
            let ts = {
                let (code, out, _) = run("date", &["+%Y%m%d-%H%M%S"]);
                if code == 0 { out.trim().to_string() } else { "0".to_string() }
            };
            let d = dest();
            let _ = run_root("mkdir", &["-p", &d]);
            let snap = format!("home-{}-{}", ts, label);
            let snap_path = format!("{}/{}", d, snap);
            let (code, _, err) = run_root(&btrfs(), &["subvolume", "snapshot", "-r", "/home", &snap_path]);
            if code != 0 { return format!("ERR snapshot failed: {}", err.trim()); }
            // prune
            prune("5");
            snap
        }
        "list" => {
            let d = dest();
            let _ = run_root("mkdir", &["-p", &d]);
            let (code, out, _) = run_root("ls", &["-1", &d]);
            if code != 0 { return "No snapshots.".to_string(); }
            let mut entries: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('.')).collect();
            if entries.is_empty() { return "No snapshots.".to_string(); }
            entries.sort_by(|a,b| b.cmp(a));
            entries.join("\n")
        }
        "restore" => {
            if args.len() < 2 { return "ERR restore needs a snapshot name".to_string(); }
            let name = &args[1];
            let d = dest();
            let snap = format!("{}/{}", d, name);
            if !std::path::Path::new(&snap).exists() { return format!("ERR snapshot not found: {}", name); }
            let rs = rsync();
            let (code, _, err) = run_root(&rs, &["-a", "--delete", &format!("{}/", snap), "/home/"]);
            if code != 0 { return format!("ERR restore failed: {}", err.trim()); }
            format!("restored {}", name)
        }
        "prune" => {
            let keep: i64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(keep_default());
            prune(&keep.to_string());
            "OK".to_string()
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run("snapshot", &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}

fn prune(keep_str: &str) {
    let keep: usize = keep_str.parse().unwrap_or(5);
    let d = dest();
    let (code, out, _) = run_root("ls", &["-1", &d]);
    if code != 0 { return; }
    let mut entries: Vec<String> = out.lines().map(|s| s.to_string()).filter(|s| !s.trim().is_empty() && !s.starts_with('.')).collect();
    entries.sort_by(|a,b| b.cmp(a));
    // delete all but newest `keep`
    if entries.len() <= keep { return; }
    for e in &entries[keep..] {
        let path = format!("{}/{}", d, e);
        let _ = run_root("btrfs", &["subvolume", "delete", &path]);
    }
}
