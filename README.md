# alexie

A Rust daemon + CLI that replaces anixie's shell-script tooling (`monitors`, `shot`, `theme`, `wallpaper`, `menu`, …) with a single long-running process. Companion to [anixie](https://github.com/s0nprem0/anixie).

## Status

Substantially complete. Native (no bash delegation):
- `monitors` — fully ported (status, list, set, scale, brightness, identify, wake, recover, internal, mirror, lid, try, keep, revert, workspaces, workspace, workspace-step, move-window, identities).
- `theme` — fully ported (list/current/next/prev/state/set/wallpaper/apply/auto/doctor/info/export/favorite/pin/wallpaper-mode/cursor-size/vars/render/presets/switch).
- `wallpaper` — native (apply/library/playback).
- `shot`/capture — native core modes (region/windows/fullscreen/window + copy/save/preview), capture-text, capture-qr, smart/scroll/redact/text/annotate via `satty`. Cursor included; cross-filesystem safe; notifies on failure.
- `toggle` — nightlight/idle/dnd/stayawake.
- `screentime` — native SQLite-backed tracking (`screentime watch` records active-window sessions; show/json/reset/path). DB at `$XDG_STATE_HOME/anixie/screentime.db`, shared with anixie's quickshell via a daily JSON file.
- `snapshot`, `webapp` — ported.
- `clipboard` — copy/list/text/image/delete/wipe/open (auto-starts the watcher; dedups history).
- `emoji`, `reminder`, `pick` — native (quickshell IPC / systemd timers).
- `record`, `webcam-overlay` — native.
- `transcode` — ported (picture/video resize + clipboard URI).
- `power` — fully ported including udev watcher (no bash delegation).
- `status` — network/sysmon/audio/bluetooth as text or JSON (P0 from doty's `wabi`). `alexie status <name> --json` (aliases `alexie network|sysmon|audio|bluetooth`) emits wabi-compatible JSON for Quickshell popups; cached to `$XDG_CACHE_HOME/anixie/`.
- `shot db` — screenshot/recording asset index (P1, wabi `media_db` port). `add/list/remove/clear/check/path/thumbs`, `color` + `ocr` groups, `status --json` (wabi `MediaStatus`); saves + recordings auto-index. DB at `$XDG_STATE_HOME/anixie/assets.db`, thumbs at `$XDG_CACHE_HOME/anixie/thumbnails/`.
- `apps` — app launcher backend (P1, wabi `apps.rs` port). `list [--json]` (wabi `{most_used,all_apps,web_history,file_history,bookmarks}`), `launch`, `bookmark`, `history`, `web search/open`, `files index/search/open`; wabi `--launch`/`--add-bookmark`/… flags accepted. DB at `$XDG_STATE_HOME/anixie/launcher.db`.

- `shortcuts`, `menu`, `focus` — quickshell IPC wrappers (shell owns UI/timer).
- `share` — native subcommands (uri/path/edit/folder/compress); interactive picker delegates.

The daemon listens on `$XDG_RUNTIME_DIR/alexie/daemon.sock` and exposes a DBus service (`org.alexie.Daemon`, typed methods/properties/signals).

## Build

```
nix build .#default
# or
cargo build --release
```

## Run

```
alexie daemon &          # start the daemon
alexie ping              # pong
alexie monitors status
alexie theme list
alexie screentime watch &  # track active-window time
alexie screentime show
alexie status sysmon --json  # CPU/RAM/disk JSON
alexie status network --json # wifi/IP JSON (wabi-compatible)
alexie shot db list --json   # screenshot index (wabi media_db)
alexie apps list --json      # launcher data (wabi get_apps_list)
```

## Integration with anixie

Enabled via `anixie.daemon.enable = true;` (home-manager option). When enabled, bash tools are renamed to `*-legacy` and alexie provides shims (`monitors`, `theme`, `shot`, …) that exec `alexie <cmd>`. Delegation resolves to `*-legacy` binaries so unported features keep working.

## Design notes

- Dependencies: serde, serde_json, toml, zbus, and rusqlite (bundled) for screentime. Build is offline-friendly once the crate cache is populated.
- Process helpers are centralized in `src/proc.rs` (`resolve_tool`, `run`, `shell_quote`).
- Protocol is line-based text (`CMD args\n` → response lines + `__END__`).
- State files shared with anixie (`XDG_STATE_HOME/anixie/`).
- Test suite: `cargo test` (unit tests for monitors, theme, wallpaper, clipboard).
