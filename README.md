# anixied

A Rust daemon + CLI that replaces anixie's shell-script tooling (`monitors`, `shot`, `theme`, `wallpaper`, `menu`, …) with a single long-running process. Companion to [anixie](https://github.com/s0nprem0/anixie).

## Status

Proof of concept. Currently implements:
- `ping`
- `monitors status`
- `monitors set <output> <key> <value>` (scale, transform, disabled, mirror, position, mode)

The daemon listens on `$XDG_RUNTIME_DIR/anixie/daemon.sock` and uses a newline-delimited text protocol.

## Build

```
nix build .#default
# or
cargo build --release
```

## Run

```
anixied daemon &          # start the daemon
anixied ping              # pong
anixied monitors status
anixied monitors set eDP-1 scale 1.5
```

## Integration with anixie (planned)

Add to anixie's `flake.nix` inputs:

```nix
anixied = {
  url = "path:/home/son/anixied";   # or github:s0nprem0/anixied
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Then a Home Manager module to install the package and run the daemon:

```nix
systemd.user.services.anixied = {
  Unit.Description = "anixied daemon";
  Service = {
    ExecStart = "${pkgs.anixied}/bin/anixied daemon";
    Restart = "on-failure";
  };
  Install.WantedBy = [ config.wayland.systemd.target ];
};
home.packages = [ pkgs.anixied ];
```

Over time, anixie's Hyprland binds and shell IPC would call `anixied <cmd>` instead of the bash scripts, migrating command-by-command. The daemon owns state (`display.conf`, theme, capture), so reloads are instant and consistent.

## Roadmap

- Port `monitors` fully (lid, brightness, workspaces sync).
- Port `theme`/`wallpaper` (palette math, matugen, caching).
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