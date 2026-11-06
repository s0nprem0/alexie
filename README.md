# alexie

A Rust daemon + CLI that replaces anixie's shell-script tooling (`monitors`, `shot`, `theme`, `wallpaper`, `menu`, …) with a single long-running process. Companion to [anixie](https://github.com/s0nprem0/anixie).

## Status

Proof of concept. Currently implements:
- `ping`
- `monitors` fully (status, list, set, scale, brightness, identify, wake, recover, internal, mirror, lid, try, keep, revert, workspaces, workspace, workspace-step, move-window, identities)

The daemon listens on `$XDG_RUNTIME_DIR/alexie/daemon.sock` and uses a newline-delimited text protocol. State files are read from `$XDG_STATE_HOME/anixie/` (shared with anixie).

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
alexie monitors set eDP-1 scale 1.5
```

## Integration with anixie (planned)

Add to anixie's `flake.nix` inputs:

```nix
alexie = {
  url = "path:/home/son/alexie";   # or github:s0nprem0/alexie
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then a Home Manager module to install the package and run the daemon:

```nix
systemd.user.services.alexie = {
  Unit.Description = "alexie daemon";
  Service = {
    ExecStart = "${pkgs.alexie}/bin/alexie daemon";
    Restart = "on-failure";
  };
  Install.WantedBy = [ config.wayland.systemd.target ];
};
home.packages = [ pkgs.alexie ];
```

Over time, anixie's Hyprland binds and shell IPC would call `alexie <cmd>` instead of the bash scripts, migrating command-by-command. The daemon owns state (`display.conf`, theme, capture), so reloads are instant and consistent.

## Roadmap

- `monitors` fully ported (native, no bash delegation).
- Port `theme`/`wallpaper` (list/current/next/prev/state done; set/wallpaper delegate to bash).
- Port `shot`/capture (freeze, region, window snap, swappy hand-off).
- Port `menu` (fast, structured, no jq).
- Port `clipboard`, `emoji`, `focus`, `screentime`, `toggle`, `power`, `snapshot`, `webapp`.
- DBus interface (optional, for richer shell integration).
- Tests (unit + property tests for parsers).

## Design notes

- std-only (no external crates) for now, to keep the build offline-friendly.
- Protocol is line-based text (`CMD args\n` → response lines + `__END__`).
- State files are shared with anixie (same `XDG_STATE_HOME/anixie/`), so the daemon and the bash tools can coexist during migration.
- No testing yet (per plan); add later.