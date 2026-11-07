//! Apps launcher: desktop apps, usage, bookmarks, history.
//!
//! Split from a single 1088-line `apps.rs`:
//! - `types` — AppInfo/WebHistoryItem/FileHistoryItem/FileIndexEntry/BookmarkItem/AppsResponse
//! - `paths` — env tool overrides, db/cache dirs
//! - `url` — url_encode/normalize_url, web-search triggers, desktop parsing
//! - `db` — open/schema/migration + bookmarks/history/usage CRUD
//! - `collect` — desktop app collection + response aggregation
//! - `launch` — browser/file launching, file index/search, rendering
//!
//! `handle()` below is just argument parsing/dispatch.

pub(crate) mod types;
pub(crate) mod paths;
pub(crate) mod url;
pub(crate) mod db;
pub(crate) mod collect;
pub(crate) mod launch;

#[allow(unused_imports)] // re-exported for public API compat
pub use types::{AppInfo, AppsResponse, BookmarkItem, FileHistoryItem, FileIndexEntry, WebHistoryItem};

use collect::collect_response;
use db::{add_bookmark, add_web_history, clear_bookmarks, clear_file_history, clear_web_history, delete_bookmark, get_bookmarks, get_file_history, get_web_history, increment_app_usage, open_db};
use launch::{index_files, open_browser, open_file, render_apps, search_files};
use url::parse_web_search;

#[cfg(test)]
use rusqlite::Connection;
#[cfg(test)]
use db::{add_file_history, get_usage_map, init_schema};
#[cfg(test)]
use url::{normalize_url, parse_desktop_file, url_encode};

pub(crate) fn usage() -> String {
    "apps — app launcher backend (desktop apps, bookmarks, history).\n\nUsage:\n  alexie apps [list] [--json] [--search TEXT]\n  alexie apps launch <name>               # record usage\n  alexie apps bookmark add <url> | remove <url> | list [--json] | clear\n  alexie apps history web clear | file clear | file list [--json]\n  alexie apps web search <query...>     # parse !g/!yt/… and open\n  alexie apps web open <query> <engine> <url>\n  alexie apps files index | search <query> | open <path> | history\n  # wabi flags: --launch NAME, --add-bookmark URL, --delete-bookmark URL,\n  # --clear-bookmarks, --clear-history, --clear-file-history, --index-files,\n  # --search-files QUERY, --open-file PATH, --file-history,\n  # --web-search QUERY, --web-search-item Q E U\n".to_string()
}

pub fn handle(args: &[String]) -> String {
    // Wabi flag aliases.
    if let Some(flag) = args.first().map(|s| s.as_str())
        && flag.starts_with("--") {
            return handle_wabi_flag(args);
        }

    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "-h" | "--help" | "help" => usage(),
        "" | "list" => {
            let want_json = args.iter().any(|a| a == "--json");
            let search = {
                let mut s = None;
                let mut i = 0;
                while i < args.len() {
                    if args[i] == "--search" && i + 1 < args.len() {
                        s = Some(args[i + 1].clone());
                    }
                    i += 1;
                }
                s
            };
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            let mut resp = collect_response(&conn);
            if let Some(q) = search {
                let ql = q.to_lowercase();
                resp.all_apps.retain(|a| a.name.to_lowercase().contains(&ql) || a.exec.to_lowercase().contains(&ql));
                resp.most_used.retain(|a| a.name.to_lowercase().contains(&ql) || a.exec.to_lowercase().contains(&ql));
            }
            if want_json {
                serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
            } else {
                render_apps(&resp)
            }
        }
        "launch" => {
            if args.len() < 2 {
                return "ERR usage: apps launch <name>".to_string();
            }
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            match increment_app_usage(&conn, &args[1]) {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR {}", e),
            }
        }
        "bookmark" => {
            let bsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            match bsub {
                "add" => {
                    if args.len() < 3 {
                        return "ERR usage: apps bookmark add <url>".to_string();
                    }
                    match add_bookmark(&conn, &args[2]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
                }
                "remove" | "delete" => {
                    if args.len() < 3 {
                        return "ERR usage: apps bookmark remove <url>".to_string();
                    }
                    match delete_bookmark(&conn, &args[2]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
                }
                "list" => {
                    let want_json = args.iter().any(|a| a == "--json");
                    let items = get_bookmarks(&conn);
                    if want_json {
                        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                    } else if items.is_empty() {
                        "bookmarks: none".to_string()
                    } else {
                        let rows: Vec<Vec<String>> = items.iter().map(|b| vec![b.name.clone(), b.url.clone()]).collect();
                        crate::table::render(&["Name", "URL"], &rows)
                    }
                }
                "clear" => match clear_bookmarks(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                _ => format!("ERR unknown subcommand: bookmark {}", bsub),
            }
        }
        "history" => {
            let hsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
            match hsub {
                "web" => {
                    let wsub = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    match wsub {
                        "clear" => match clear_web_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                        "list" => {
                            let want_json = args.iter().any(|a| a == "--json");
                            let items = get_web_history(&conn);
                            if want_json {
                                serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                            } else if items.is_empty() {
                                "web history: none".to_string()
                            } else {
                                let rows: Vec<Vec<String>> = items.iter().map(|h| vec![h.query.clone(), h.engine.clone(), h.url.clone()]).collect();
                                crate::table::render(&["Query", "Engine", "URL"], &rows)
                            }
                        }
                        _ => format!("ERR unknown subcommand: history web {}", wsub),
                    }
                }
                "file" => {
                    let fsub = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    match fsub {
                        "clear" => match clear_file_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
                        "list" => {
                            let want_json = args.iter().any(|a| a == "--json");
                            let items = get_file_history(&conn);
                            if want_json {
                                serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                            } else if items.is_empty() {
                                "file history: none".to_string()
                            } else {
                                let rows: Vec<Vec<String>> = items.iter().map(|h| vec![h.path.clone(), h.name.clone()]).collect();
                                crate::table::render(&["Path", "Name"], &rows)
                            }
                        }
                        _ => format!("ERR unknown subcommand: history file {}", fsub),
                    }
                }
                _ => "ERR usage: apps history <web|file> <clear|list>".to_string(),
            }
        }
        "web" => {
            let wsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match wsub {
                "search" => {
                    if args.len() < 3 {
                        return "ERR usage: apps web search <query...>".to_string();
                    }
                    let query = args[2..].join(" ");
                    let Some(item) = parse_web_search(&query) else {
                        return "ERR web search needs a !-trigger (e.g. !g rust)".to_string();
                    };
                    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
                    if let Err(e) = add_web_history(&conn, &item) {
                        eprintln!("Error saving web search history: {}", e);
                    }
                    open_browser(&item);
                    item.url
                }
                "open" => {
                    if args.len() < 5 {
                        return "ERR usage: apps web open <query> <engine> <url>".to_string();
                    }
                    let item = WebHistoryItem { query: args[2].clone(), engine: args[3].clone(), url: args[4].clone() };
                    if item.query.trim().is_empty() || item.url.trim().is_empty() {
                        return "ERR web open needs query and url".to_string();
                    }
                    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
                    if let Err(e) = add_web_history(&conn, &item) {
                        eprintln!("Error saving web search history: {}", e);
                    }
                    open_browser(&item);
                    item.url
                }
                _ => "ERR usage: apps web <search|open> …".to_string(),
            }
        }
        "files" => {
            let fsub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match fsub {
                "index" => index_files(),
                "search" => {
                    if args.len() < 3 {
                        return "ERR usage: apps files search <query>".to_string();
                    }
                    let query = args[2..].join(" ");
                    search_files(&query)
                }
                "open" => {
                    if args.len() < 3 {
                        return "ERR usage: apps files open <path>".to_string();
                    }
                    open_file(&args[2])
                }
                "history" => {
                    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
                    let items = get_file_history(&conn);
                    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
                }
                _ => "ERR usage: apps files <index|search|open|history> …".to_string(),
            }
        }
        _ => format!("ERR unknown subcommand: {}", sub),
    }
}

fn handle_wabi_flag(args: &[String]) -> String {
    let flag = args[0].as_str();
    let Ok(conn) = open_db() else { return "ERR db: open failed".to_string() };
    match flag {
        "--clear-history" => match clear_web_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
        "--clear-file-history" => match clear_file_history(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
        "--clear-bookmarks" => match clear_bookmarks(&conn) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) },
        "--add-bookmark" => {
            if args.len() < 2 {
                return "ERR usage: apps --add-bookmark <url>".to_string();
            }
            match add_bookmark(&conn, &args[1]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
        }
        "--delete-bookmark" => {
            if args.len() < 2 {
                return "ERR usage: apps --delete-bookmark <url>".to_string();
            }
            match delete_bookmark(&conn, &args[1]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
        }
        "--launch" => {
            if args.len() < 2 {
                return "ERR usage: apps --launch <name>".to_string();
            }
            match increment_app_usage(&conn, &args[1]) { Ok(()) => "OK".to_string(), Err(e) => format!("ERR {}", e) }
        }
        "--index-files" => index_files(),
        "--search-files" => {
            if args.len() < 2 {
                return "ERR usage: apps --search-files <query>".to_string();
            }
            search_files(&args[1])
        }
        "--open-file" => {
            if args.len() < 2 {
                return "ERR usage: apps --open-file <path>".to_string();
            }
            open_file(&args[1])
        }
        "--file-history" => serde_json::to_string(&get_file_history(&conn)).unwrap_or_else(|_| "[]".to_string()),
        "--web-search" => {
            if args.len() < 2 {
                return "ERR usage: apps --web-search <query>".to_string();
            }
            let Some(item) = parse_web_search(&args[1]) else {
                return "ERR web search needs a !-trigger".to_string();
            };
            if let Err(e) = add_web_history(&conn, &item) {
                eprintln!("Error saving web search history: {}", e);
            }
            open_browser(&item);
            "OK".to_string()
        }
        "--web-search-item" => {
            if args.len() < 4 {
                return "ERR usage: apps --web-search-item <query> <engine> <url>".to_string();
            }
            let item = WebHistoryItem { query: args[1].clone(), engine: args[2].clone(), url: args[3].clone() };
            if item.query.trim().is_empty() || item.url.trim().is_empty() {
                return "ERR web search item needs query and url".to_string();
            }
            if let Err(e) = add_web_history(&conn, &item) {
                eprintln!("Error saving web search history: {}", e);
            }
            open_browser(&item);
            "OK".to_string()
        }
        _ => format!("ERR unknown flag: {}", flag),
    }
}

/// JSON for D-Bus.
pub(crate) fn list_json() -> String {
    match open_db() {
        Ok(conn) => {
            let resp = collect_response(&conn);
            serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
        }
        Err(_) => serde_json::json!({ "most_used": [], "all_apps": [], "web_history": [], "file_history": [], "bookmarks": [] }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_normalize_url() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("example.com/"), "https://example.com");
        assert_eq!(
            normalize_url("https://Mail.Google.com/Mail/u/0/"),
            "https://mail.google.com/Mail/u/0"
        );
        assert_eq!(normalize_url("http://foo.bar/"), "http://foo.bar");
    }

    #[test]
    fn test_parse_web_search() {
        let item = parse_web_search("!g rust").unwrap();
        assert_eq!(item.engine, "google");
        assert_eq!(item.query, "rust");
        assert_eq!(item.url, "https://www.google.com/search?q=rust");
        let yt = parse_web_search("!yt lofi").unwrap();
        assert_eq!(yt.engine, "youtube");
        assert!(yt.url.contains("search_query=lofi"));
        assert!(parse_web_search("rust").is_none());
        assert!(parse_web_search("!g").is_none());
    }

    #[test]
    fn url_encode_keeps_safe_chars() {
        assert_eq!(url_encode("a b/c"), "a+b%2Fc");
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn parse_desktop_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "alexie-apps-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("kitty.desktop");
        std::fs::write(&p, "[Desktop Entry]\nName=Kitty\nExec=kitty --single-instance %U\nIcon=kitty\n").unwrap();
        let app = parse_desktop_file(&p).unwrap();
        assert_eq!(app.name, "Kitty");
        assert_eq!(app.exec, "kitty --single-instance");
        assert_eq!(app.icon, "kitty");

        // NoDisplay hides
        let p2 = dir.join("hidden.desktop");
        std::fs::write(&p2, "[Desktop Entry]\nName=Hidden\nExec=x\nNoDisplay=true\n").unwrap();
        assert!(parse_desktop_file(&p2).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bookmark_roundtrip_normalizes() {
        let conn = open_mem();
        add_bookmark(&conn, "example.com/docs/").unwrap();
        let items = get_bookmarks(&conn);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://example.com/docs");
        delete_bookmark(&conn, "https://example.com/docs").unwrap();
        assert!(get_bookmarks(&conn).is_empty());
    }

    #[test]
    fn usage_increments_and_collects() {
        let conn = open_mem();
        increment_app_usage(&conn, "Kitty").unwrap();
        increment_app_usage(&conn, "Kitty").unwrap();
        let map = get_usage_map(&conn);
        assert_eq!(map.get("Kitty"), Some(&2));
    }

    #[test]
    fn web_and_file_history_roundtrips() {
        let conn = open_mem();
        let item = WebHistoryItem { query: "rust".into(), engine: "google".into(), url: "https://www.google.com/search?q=rust".into() };
        add_web_history(&conn, &item).unwrap();
        assert_eq!(get_web_history(&conn).len(), 1);
        add_file_history(&conn, "/home/u/doc.txt").unwrap();
        assert_eq!(get_file_history(&conn)[0].name, "doc.txt");
        clear_web_history(&conn).unwrap();
        clear_file_history(&conn).unwrap();
        assert!(get_web_history(&conn).is_empty());
        assert!(get_file_history(&conn).is_empty());
    }

    #[test]
    fn collect_response_shapes_match_wabi() {
        let conn = open_mem();
        let resp = collect_response(&conn);
        let v = serde_json::to_value(&resp).unwrap();
        for k in ["most_used", "all_apps", "web_history", "file_history", "bookmarks"] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
    }

    #[test]
    fn unknown_subcommand_fails_fast() {
        let out = handle(&["__bogus__".to_string()]);
        assert!(out.starts_with("ERR unknown subcommand:"), "got: {out}");
    }

    #[test]
    fn usage_lists_commands() {
        let u = usage();
        assert!(u.contains("apps"));
        assert!(u.contains("--launch"));
    }
}
