# alexie

A Rust daemon + CLI that replaces anixie's shell-script tooling (`monitors`, `shot`, `theme`, `wallpaper`, `menu`, …) with a single long-running process. Companion to [anixie](https://github.com/s0nprem0/anixie).

## Status

Substantially complete. Native (no bash delegation):
- `monitors` — fully ported (status, list, set, scale, brightness, identify, wake, recover, internal, mirror, lid, try, keep, revert, workspaces, workspace, workspace-step, move-window, identities).
- `theme` — list/current/next/prev/state (set/wallpaper delegate to legacy bash).
- `wallpaper` — current (heavy ops delegate).
- `shot`/capture — core modes (region/windows/fullscreen/window + copy/save/preview), capture-text, capture-qr (smart/scroll/redact/text/annotate delegate).
- `toggle` — nightlight/idle/dnd/stayawake.
- `focus`, `screentime` — ported.
- `snapshot`, `webapp` — ported.
- `clipboard` — copy/list/text/image/delete/wipe/open.
- `emoji`, `shortcuts`, `reminder`, `pick`, `menu` — thin wrappers.
- `power` — fully ported including udev watcher (no bash delegation).

Delegated to legacy bash (complex process/systemd management): `record`, `transcode`, `webcam-overlay`, `share`.

The daemon listens on `$XDG_RUNTIME_DIR/alexie/daemon.sock` and exposes a DBus service (`org.alexie.Daemon`, typed methods/properties/signals). It also ships an `anixie` front-door binary replacing the bash dispatcher.

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
```

## Integration with anixie

Enabled via `anixie.daemon.enable = true;` (home-manager option). When enabled, bash tools are renamed to `*-legacy` and alexie provides shims (`monitors`, `theme`, `shot`, …) that exec `alexie <cmd>`. Delegation resolves to `*-legacy` binaries so unported features keep working.

## Design notes

- std-only (no external crates except zbus) to keep the build offline-friendly (cargo network used only for zbus).
- Protocol is line-based text (`CMD args\n` → response lines + `__END__`).
- State files shared with anixie (`XDG_STATE_HOME/anixie/`).
- No test suite yet (planned).
