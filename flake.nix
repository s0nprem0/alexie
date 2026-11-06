{
  description = "alexie: Rust daemon + CLI for anixie";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "alexie";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # No test suite yet; skip the (empty) check phase so builds don't
          # pay for a second full compile pass.
          doCheck = false;
          # zbus and the rest of the dependency tree are pure Rust, but keep
          # pkg-config on the path so any future native crate resolves cleanly.
          nativeBuildInputs = [ pkgs.pkg-config ];
        };
        devShells.default = pkgs.mkShell {
          buildInputs = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.rustfmt
            pkgs.gcc
            pkgs.pkg-config
          ];
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });
}