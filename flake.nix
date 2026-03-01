{
  description = "Agent Box - Git repository management tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    (flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        mdbook-excalidraw = pkgs.rustPlatform.buildRustPackage rec {
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

        wrappers = pkgs.rustPlatform.buildRustPackage {
          pname = "agent-wrappers";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          cargoBuildFlags = [ "-p" "agent-wrappers" ];
          cargoTestFlags = [ "-p" "agent-wrappers" ];
          cargoInstallFlags = [ "-p" "agent-wrappers" ];

          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        };

        portal = pkgs.rustPlatform.buildRustPackage {
          pname = "agent-portal";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          cargoBuildFlags = [ "-p" "agent-portal" ];
          cargoTestFlags = [ "-p" "agent-portal" ];
          cargoInstallFlags = [ "-p" "agent-portal" ];

          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        };
      in
      {
        packages = {
          wrappers = wrappers;
          portal = portal;
          mdbook-excalidraw = pkgs.mdbook-excalidraw;
          default = wrappers;
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
            # Rust toolchain
            cargo
            rustc
            rustfmt
            clippy

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
