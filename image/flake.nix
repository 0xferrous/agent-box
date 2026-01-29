{
  description = "Agent Box - Docker image with nix and packages";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nix.url = "github:0xferrous/nix/extra-args";
    nix-ai-tools.url = "github:numtide/nix-ai-tools";
  };

  outputs =
    {
      self,
      nixpkgs,
      nix,
      nix-ai-tools,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forEachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          id = if builtins.pathExists ./id.nix then import ./id.nix else { };
          uid = id.uid;
          gid = id.gid;
          uname = id.uname;
          gname = id.gname;
          aiTools = nix-ai-tools.packages.${system};

          userHome = if uid == 0 then "/root" else "/home/${uname}";

          # Entrypoint script that runs `nix develop --command <args>` or `nix develop --command bash` if no args
          entrypoint = pkgs.writeShellScriptBin "entrypoint" ''
            if [ $# -eq 0 ]; then
              exec nix develop --command bash
            else
              exec nix develop --command "$@"
            fi
          '';

          buildImage =
            {
              packages ? [ ],
            }:
            pkgs.callPackage "${nix}/docker.nix" {
              name = "agent-box";
              tag = "latest";

              inherit
                uid
                gid
                uname
                gname
                ;

              nixConf = {
                experimental-features = [
                  "nix-command"
                  "flakes"
                ];
              };

              extraPkgs = packages ++ [ entrypoint ];

              Cmd = [ "${entrypoint}/bin/entrypoint" ];

              # extraFakeRootCommands = ''
              #   mkdir -p .${userHome}/.gnupg
              #   chmod 700 .${userHome}/.gnupg
              #   chown ${toString uid}:${toString gid} .${userHome}/.gnupg
              # '';
            };

          # Default package list
          defaultPackages = import ./packages.nix { inherit pkgs aiTools; };
        in
        {
          default = buildImage { packages = defaultPackages; };

          # Expose the builder function for custom packages/user
          custom = buildImage;
        }
      );

      devShells = forEachSystem (
        system:
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
