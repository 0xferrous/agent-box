{
  description = "Agent Box - Git repository management tool";

  nixConfig = {
    extra-substituters = [ "https://0xferrous.cachix.org" ];
    extra-trusted-public-keys = [
      "0xferrous.cachix.org-1:p38oLu+8I+EwBW6NCl+ffI8qn+WOtUeRzES/dYNuSUE="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    (flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rustfmt" "clippy" ];
        };

        workspaceToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        wrappersCrateToml = builtins.fromTOML (builtins.readFile ./wrappers/Cargo.toml);
        portalCrateToml = builtins.fromTOML (builtins.readFile ./portal/Cargo.toml);
        abCrateToml = builtins.fromTOML (builtins.readFile ./ab/Cargo.toml);

        portalHostBin = "${portalCrateToml.package.name}-host";

        mdbook-excalidraw = pkgs.rustPlatform.buildRustPackage {
          pname = "mdbook-excalidraw";
          version = "0.1.0";
          src = pkgs.fetchFromGitHub {
            # inherit pname version;
            owner = "peachycloudsecurity";
            repo = "mdbook-excalidraw";
            rev = "2d8f07905f57d1c460ccb9f7279af4f4999b9ee2";
            sha256 = "sha256-Sf2cWaoZ3tOjRWaOp898RME6+7uLYv9gb7RTsg76ETU=";
          };
          cargoHash = "sha256-h+sunASiueLa1LZNfTUZlidS1KVh9orxFnTpCcf3s/Y=";
        };

        wrappers = pkgs.rustPlatform.buildRustPackage rec {
          pname = wrappersCrateToml.package.name;
          version = workspaceToml.workspace.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          cargoBuildFlags = [ "-p" pname ];
          cargoTestFlags = [ "-p" pname ];
          cargoInstallFlags = [ "-p" pname ];

          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        };

        portal = pkgs.rustPlatform.buildRustPackage rec {
          pname = portalCrateToml.package.name;
          version = workspaceToml.workspace.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          cargoBuildFlags = [ "-p" pname "--bin" portalHostBin ];
          cargoTestFlags = [ "-p" pname "--bin" portalHostBin ];
          cargoInstallFlags = [ "-p" pname "--bin" portalHostBin ];

          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        };

        cli = pkgs.rustPlatform.buildRustPackage rec {
          pname = "${portalCrateToml.package.name}-cli";
          version = workspaceToml.workspace.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          cargoBuildFlags = [ "-p" portalCrateToml.package.name "--bin" pname ];
          cargoTestFlags = [ "-p" portalCrateToml.package.name "--bin" pname ];
          cargoInstallFlags = [ "-p" portalCrateToml.package.name "--bin" pname ];

          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        };

        ab = pkgs.rustPlatform.buildRustPackage rec {
          pname = abCrateToml.package.name;
          version = workspaceToml.workspace.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" pname ];
          cargoTestFlags = [ "-p" pname ];
        };
      in
      {
        packages = {
          inherit ab wrappers portal cli mdbook-excalidraw;
          default = ab;
        };

        apps = {
          wrappers = {
            type = "app";
            program = "${wrappers}/bin/agent-portal-client";
          };

          wl-paste-wrapper = {
            type = "app";
            program = "${wrappers}/bin/wl-paste";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain (match CI's stable channel)
            rustToolchain

            # Build dependencies
            pkg-config
            openssl

            # Documentation tools
            mdbook
            mdbook-excalidraw

            # Additional tools
            git
          ];

          shellHook = ''
            echo "Agent Box development environment"
            echo "Rust version: $(rustc --version)"
          '';

          # Environment variables for OpenSSL
          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        };
      }
    ))
    // {
      homeManagerModules = {
        agent-portal = import ./nix/home-manager/agent-portal.nix { inherit self; };
        default = self.homeManagerModules.agent-portal;
      };
    };
}
