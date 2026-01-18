{
  description = "Agent Box - Git repository management tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
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
    );
}
