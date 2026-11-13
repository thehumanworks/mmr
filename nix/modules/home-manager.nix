# home-manager module: `programs.mmr` installs and configures the CLI for one
# user; `services.mmr-mcp` runs the HTTP MCP server as a systemd user service
# on Linux or a launchd agent on macOS.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  common = import ./common.nix { inherit lib pkgs; };

  cfg = config.programs.mmr;
  mcp = config.services.mmr-mcp;

  inherit (pkgs.stdenv.hostPlatform) isDarwin isLinux;

  mcpEnvironment = lib.optionalAttrs cfg.enable (common.cliEnvironment cfg) // mcp.environment;

  # systemd expands `%` specifiers and needs quotes around values with spaces.
  systemdEnvironment = lib.mapAttrsToList (
    name: value: ''"${name}=${lib.escape [ "\\" "\"" ] (lib.replaceStrings [ "%" ] [ "%%" ] value)}"''
  ) mcpEnvironment;

  # launchd has no EnvironmentFile, so source the file before exec.
  launchdProgram =
    if mcp.environmentFile == null then
      common.mcpCommand mcp
    else
      [
        "${pkgs.writeShellScript "mmr-mcp" ''
          set -a
          . ${lib.escapeShellArg mcp.environmentFile}
          set +a
          exec ${lib.escapeShellArgs (common.mcpCommand mcp)}
        ''}"
      ];
in
{
  options.programs.mmr = {
    enable = lib.mkEnableOption "mmr, the local AI coding-session history CLI";

    package = common.packageOption;

    installSkill = lib.mkEnableOption "" // {
      description = ''
        Whether to link the bundled mmr agent skill into
        {file}`~/.agents/skills/mmr`. These are the same files that
        `mmr skill install` writes, kept in step with the installed package.
      '';
    };
  }
  // common.programOptions;

  options.services.mmr-mcp = common.mcpOptions {
    defaultPackage = cfg.package;
    defaultPackageText = "config.programs.mmr.package";
    environmentFileDescription = ''
      On macOS the file is read with `sh`, so keep to plain `NAME=value`
      lines.
    '';
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      warnings = common.removedWarnings "programs.mmr" cfg;

      home.packages = [ cfg.package ];

      home.sessionVariables = common.cliEnvironment cfg;

      home.file.".agents/skills/mmr" = lib.mkIf cfg.installSkill {
        source = "${cfg.package}/share/mmr/skills/mmr";
        recursive = true;
      };
    })

    (lib.mkIf mcp.enable {
      warnings = common.mcpWarnings "services.mmr-mcp" mcp;

      assertions = [
        {
          assertion = isLinux || isDarwin;
          message = "services.mmr-mcp needs systemd (Linux) or launchd (macOS).";
        }
      ];

      systemd.user.services.mmr-mcp = lib.mkIf isLinux {
        Unit = {
          Description = "mmr MCP server (streamable HTTP)";
          Documentation = "https://github.com/thehumanworks/mmr";
        };
        Service = {
          ExecStart = lib.escapeShellArgs (common.mcpCommand mcp);
          Environment = systemdEnvironment;
          Restart = "on-failure";
          # mmr shuts its HTTP listener down cleanly on SIGINT.
          KillSignal = "SIGINT";
        }
        // lib.optionalAttrs (mcp.environmentFile != null) {
          EnvironmentFile = mcp.environmentFile;
        };
        Install.WantedBy = [ "default.target" ];
      };

      launchd.agents.mmr-mcp = lib.mkIf isDarwin {
        enable = true;
        config = {
          ProgramArguments = launchdProgram;
          EnvironmentVariables = mcpEnvironment;
          RunAtLoad = true;
          KeepAlive = {
            Crashed = true;
            SuccessfulExit = false;
          };
          ProcessType = "Background";
          StandardOutPath = "${config.home.homeDirectory}/Library/Logs/mmr-mcp.log";
          StandardErrorPath = "${config.home.homeDirectory}/Library/Logs/mmr-mcp.log";
        };
      };
    })
  ];
}
