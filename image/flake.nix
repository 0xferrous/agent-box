{
  description = "Agent Box - Docker image with nix and packages";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nix.url = "github:nixos/nix/2.33.1";
  };

  outputs = { self, nixpkgs, nix }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forEachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          id = import ./id.nix;
          uid = id.uid;
          gid = id.gid;
          uname = id.uname;
          gname = id.gname;

          nixImage = pkgs.callPackage "${nix}/docker.nix" id;

          buildImage = { packages ? [] } : pkgs.callPackage "${nix}/docker.nix" {
            name = "agent-box";
            tag = "latest";

            inherit uid gid uname gname;

            extraPkgs = packages;
          };

          # Build the image with specified packages layered on top of nix base
          # buildImage = {
          #   packages ? [],
          # }: pkgs.dockerTools.buildLayeredImage {
          #   name = "agent-box";
          #   tag = "latest";
          #
          #   inherit uid gid uname gname;
          #
          #   # Use nix docker image as base layer
          #   fromImage = nixImage;
          #
          #   # Include packages in the image
          #   contents = packages;
          #
          #   config = {
          #     User = "${toString uid}:${toString gid}";
          #     Cmd = [ "/bin/bash" ];
          #   };
          # };

          # Default package list
          defaultPackages = with pkgs; [
            bash
            curl
            wget
            jq
            ripgrep
            fd
            tree
            neovim
            jujutsu
            strace
          ];
        in
        {
          default = buildImage { packages = defaultPackages; };

          # Expose the builder function for custom packages/user
          custom = buildImage;
        }
      );

      devShells = forEachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              docker
              nixfmt-rfc-style
            ];
          };
        }
      );
    };
}
