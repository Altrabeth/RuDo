{
  description = "A keyboard-driven terminal UI for todo lists stored as Markdown files";

  # The Rust toolchain comes from rust-overlay at the version pinned in rust-toolchain.toml, so
  # the package builds with the compiler RuDo is tested with, whatever nixpkgs ships.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forEachSystem =
        function:
        lib.genAttrs systems (
          system:
          function (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
      cargoPackage = (lib.importTOML ./Cargo.toml).package;
      toolchainFile = ./rust-toolchain.toml;
      toolchainChannel = (lib.importTOML toolchainFile).toolchain.channel;
    in
    {
      packages = forEachSystem (
        pkgs:
        let
          rust = pkgs.rust-bin.stable.${toolchainChannel}.minimal;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = cargoPackage.name;
            inherit (cargoPackage) version;
            # Only what the build and the tests read, so that other changes keep the hash. The
            # tests read every Markdown document: docs/ through include_str!, and all of them
            # to check that their links resolve.
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./src
                ./tests
                ./assets
                ./proptest-regressions
                ./LICENSE
                (lib.fileset.fileFilter (file: file.hasExt "md") ./.)
              ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            meta = {
              inherit (cargoPackage) description;
              homepage = cargoPackage.repository;
              license = lib.licenses.gpl3Plus;
              mainProgram = "rudo";
              platforms = systems;
            };
          };
        }
      );

      apps = forEachSystem (pkgs: {
        default = {
          type = "app";
          program = lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          meta.description = cargoPackage.description;
        };
      });

      # `nix flake check` builds the package, which runs the tests
      checks = forEachSystem (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (pkgs.rust-bin.fromRustupToolchainFile toolchainFile)
            pkgs.just
            pkgs.cargo-deny
            pkgs.cargo-insta
            pkgs.cargo-llvm-cov
            pkgs.python3
          ];
        };
      });
    };
}
