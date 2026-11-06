use std::env;
use std::fs;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> (i32, String, String) {
    match Command::new(cmd).args(args).output() {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

fn apps_dir() -> String {
    env::var("APPS").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/.local/share/applications", home)
    })
}

fn browser() -> String {
    env::var("BROWSER").unwrap_or_else(|_| "chromium".to_string())
}

fn update_db() -> String {
    env::var("UPDATE_DB").unwrap_or_else(|_| "update-desktop-database".to_string())
}

fn slug(name: &str) -> String {
    name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect::<String>()
        .trim_matches('-').to_string()
        // collapse multiple dashes
}

fn slug_clean(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else {
            if !prev_dash && !out.is_empty() {
                out.push('-');
            }
            prev_dash = true;
        }
    }
    while out.ends_with('-') { out.pop(); }
    out
}

pub fn handle(args: &[String]) -> String {
    let sub = args.get(0).map(|s| s.as_str()).unwrap_or("");
    match sub {
        "" | "-h" | "--help" | "help" => "webapp — a website, in the application menu\n\nUsage:\n  webapp add NAME URL\n  webapp list\n  webapp remove NAME".to_string(),
        "add" => {
            if args.len() < 3 { return "ERR usage: webapp add NAME URL".to_string(); }
            let name = &args[1];
            let url = &args[2];
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return "ERR the address needs to be http:// or https://".to_string();
            }
            let id = slug_clean(name);
            if id.is_empty() { return "ERR a name needs a letter or a digit in it".to_string(); }
            let dir = apps_dir();
            let _ = std::fs::create_dir_all(&dir);
            let path = format!("{}/webapp-{}.desktop", dir, id);
            let browser_bin = browser();
            let content = format!(
                "[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nComment={}\nExec={} --app={} --class=webapp-{}\nIcon=web-browser\nTerminal=false\nCategories=Network;WebBrowser;\nStartupWMClass=webapp-{}\n",
                name, url, browser_bin, url, id, id
            );
            if std::fs::write(&path, content).is_err() {
                return "ERR could not write desktop file".to_string();
            }
            let _ = run(&update_db(), &[&dir]);
            format!("webapp: installed \"{}\" ({})", name, url)
        }
        "list" => {
            let dir = apps_dir();
            let mut found = false;
            let mut out = String::new();
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with("webapp-") && name.ends_with(".desktop") {
                        // read Name= line
                        if let Ok(content) = std::fs::read_to_string(e.path()) {
                            for line in content.lines() {
                                if let Some(rest) = line.strip_prefix("Name=") {
                                    out.push_str(rest);
                                    out.push('\n');
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if !found { "webapp: none installed".to_string() } else { out.trim_end().to_string() }
        }
        "remove" => {
            if args.len() < 2 { return "ERR usage: webapp remove NAME".to_string(); }
            let id = slug_clean(&args[1]);
            let dir = apps_dir();
            let path = format!("{}/webapp-{}.desktop", dir, id);
            if !std::path::Path::new(&path).exists() {
                return format!("ERR nothing called \"{}\"", args[1]);
            }
            let _ = std::fs::remove_file(&path);
            let _ = run(&update_db(), &[&dir]);
            format!("webapp: removed \"{}\"", args[1])
        }
        _ => {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let (code, out, err) = run("webapp", &a);
            if code == 0 { if out.trim().is_empty() { "OK".to_string() } else { out.trim_end().to_string() } }
            else { format!("ERR (exit {}) {}", code, err.trim()) }
        }
    }
}
