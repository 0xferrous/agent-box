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
          # Falls back to direct execution if no flake.nix is found (nix flake metadata searches up the directory tree)
          entrypoint = pkgs.writeShellScriptBin "entrypoint" ''
            if nix flake metadata &>/dev/null; then
              if [ $# -eq 0 ]; then
                exec nix develop --command bash
              else
                exec nix develop --command "$@"
              fi
            else
              if [ $# -eq 0 ]; then
                exec bash
              else
                exec "$@"
              fi
            fi
          '';

          buildImage =
            {
              packages ? [ ],
              directories ? [ ],
              env ? { },
            }:
            let
              # Generate chown/mkdir commands for directories
              # Each directory can be:
              #   - a string (path): creates with default 755 permissions
              #   - an attrset with path and mode: creates with specified permissions
              dirCommands = pkgs.lib.concatMapStrings (
                dir:
                let
                  path = if builtins.isString dir then dir else dir.path;
                  mode = if builtins.isString dir then "755" else dir.mode or "755";
                in
                ''
                  mkdir -p .${path}
                  chmod ${mode} .${path}
                  chown ${toString uid}:${toString gid} .${path}
                ''
              ) directories;

              # Convert env attrset to list of "KEY=value" strings for Docker Env
              envList = pkgs.lib.mapAttrsToList (name: value: "${name}=${value}") env;

              finalPackages = packages ++ [ entrypoint ];
            in
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

              extraPkgs = finalPackages;

              Entrypoint = [ "${entrypoint}/bin/entrypoint" ];
              Env = envList;

              extraFakeRootCommands = ''
                # Create /is-container marker directory owned by root with read-only permissions
                mkdir -p ./is-container
                chmod 555 ./is-container
                chown 0:0 ./is-container

                # Setup direnv in bashrc
                echo 'eval "$(direnv hook bash)"' >> ./home/${uname}/.bashrc
              ''
              + dirCommands;
            };

          # Default package list
          defaultPackages = import ./packages.nix { inherit pkgs aiTools; };
        in
        {
          default = buildImage {
            packages = defaultPackages;
            directories = [ "${userHome}/.local" "${userHome}/.cache" ];
            env = {
              EDITOR = "nvim";
            };
          };

          # Example with custom environment variables
          with-env = buildImage {
            packages = defaultPackages;
            directories = [ "${userHome}/.local" ];
            env = {
              AGENT_BOX_VERSION = "1.0";
              CONTAINER_TYPE = "agent-box";
            };
          };

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
              direnv
            ];

            shellHook = ''
              # Setup direnv
              eval "$(direnv hook bash)"
            '';
          };
        }
      );
    };
}
