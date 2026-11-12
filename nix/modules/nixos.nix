# NixOS module: `programs.mmr` installs the CLI system-wide; `services.mmr-mcp`
# runs the HTTP MCP server as a system service on behalf of one user.
{
  config,
  lib,
  pkgs,
  utils,
  ...
}:

let
  common = import ./common.nix { inherit lib pkgs; };

  cfg = config.programs.mmr;
  mcp = config.services.mmr-mcp;

  configPath = "/etc/mmr/config.json";

  cliEnvironment =
    common.cliEnvironment cfg
    // lib.optionalAttrs (cfg.settings != { }) { MMR_CONFIG_FILE = configPath; };
in
{
  options.programs.mmr = {
    enable = lib.mkEnableOption "mmr, the local AI coding-session history CLI";

    package = common.packageOption;
  }
  // common.programOptions {
    settingsDescription = ''
      When this is not empty, the file is placed at {file}`${configPath}`
      and `MMR_CONFIG_FILE` points every user at it, which replaces each
      user's own {file}`~/.config/mmr/config.json`. Use the home-manager
      module for per-user settings.
    '';
  };

  options.services.mmr-mcp =
    common.mcpOptions {
      defaultPackage = cfg.package;
      defaultPackageText = "config.programs.mmr.package";
      environmentFileDescription = ''
        It is passed to systemd as `EnvironmentFile=`, so it can stay
        readable by root only.
      '';
    }
    // {
      user = lib.mkOption {
        type = lib.types.str;
        example = "alice";
        description = ''
          Account whose coding-session history the server serves. The
          service runs as this user, with that user's home directory, and
          reads {file}`~/.config/mmr/config.json` like the CLI does.
        '';
      };
    };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      warnings = common.settingsWarnings "programs.mmr" cfg.settings;

      environment.systemPackages = [ cfg.package ];

      environment.sessionVariables = cliEnvironment;

      environment.etc."mmr/config.json" = lib.mkIf (cfg.settings != { }) {
        source = common.jsonFormat.generate "mmr-config.json" cfg.settings;
      };
    })

    (lib.mkIf mcp.enable {
      warnings = common.mcpWarnings "services.mmr-mcp" mcp;

      systemd.services.mmr-mcp = {
        description = "mmr MCP server (streamable HTTP)";
        documentation = [ "https://github.com/thehumanworks/mmr" ];
        wantedBy = [ "multi-user.target" ];
        after = [ "network.target" ];

        environment = lib.optionalAttrs cfg.enable cliEnvironment // mcp.environment;

        serviceConfig = {
          ExecStart = utils.escapeSystemdExecArgs (common.mcpCommand mcp);
          User = mcp.user;
          Restart = "on-failure";
          # mmr shuts its HTTP listener down cleanly on SIGINT.
          KillSignal = "SIGINT";
          UMask = "0077";

          # mmr reads the user's history and keeps its store under
          # ~/.local/share/mmr, so the home directory stays writable.
          CapabilityBoundingSet = "";
          LockPersonality = true;
          NoNewPrivileges = true;
          PrivateDevices = true;
          PrivateTmp = true;
          ProtectClock = true;
          ProtectControlGroups = true;
          ProtectHostname = true;
          ProtectKernelLogs = true;
          ProtectKernelModules = true;
          ProtectKernelTunables = true;
          ProtectSystem = "full";
          RestrictAddressFamilies = [
            "AF_UNIX"
            "AF_INET"
            "AF_INET6"
          ];
          RestrictNamespaces = true;
          RestrictRealtime = true;
          RestrictSUIDSGID = true;
          SystemCallArchitectures = "native";
        }
        // lib.optionalAttrs (mcp.environmentFile != null) {
          EnvironmentFile = mcp.environmentFile;
        };
      };
    })
  ];
}
