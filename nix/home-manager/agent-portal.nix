{ self }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.agent-portal;
in
{
  options.services.agent-portal = {
    enable = lib.mkEnableOption "agent portal host service";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.portal;
      defaultText = lib.literalExpression "self.packages.${pkgs.system}.portal";
      description = "Package providing the `agent-portal-host` binary.";
    };

    wrappersPackage = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.wrappers;
      defaultText = lib.literalExpression "self.packages.${pkgs.system}.wrappers";
      description = "Package providing wrapper binaries like `wl-paste`.";
    };

    installWrappers = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to install wrapper binaries into home.packages.";
    };

    socketPath = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional socket path override passed as `--socket` to agent-portal-host.";
    };

    wlPasteBinary = lib.mkOption {
      type = lib.types.str;
      default = "${pkgs.wl-clipboard}/bin/wl-paste";
      defaultText = lib.literalExpression ''"${pkgs.wl-clipboard}/bin/wl-paste"'';
      description = "Host wl-paste binary used by portal host to avoid wrapper recursion.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = lib.optionals cfg.installWrappers [ cfg.wrappersPackage ];

    systemd.user.services.agent-portal-host = {
      Unit = {
        Description = "Agent Portal Host Service";
        After = [ "graphical-session.target" ];
        Wants = [ "graphical-session.target" ];
      };

      Service = {
        Type = "simple";
        Environment = [ "AGENT_PORTAL_HOST_WL_PASTE=${cfg.wlPasteBinary}" ];
        ExecStart =
          if cfg.socketPath == null then
            "${cfg.package}/bin/agent-portal-host"
          else
            "${cfg.package}/bin/agent-portal-host --socket ${lib.escapeShellArg cfg.socketPath}";
        Restart = "on-failure";
        RestartSec = 1;
      };

      Install = {
        WantedBy = [ "default.target" ];
      };
    };
  };
}
