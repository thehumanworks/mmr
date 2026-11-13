# Builds a home-manager generation with every mmr option set and checks the
# files it writes.
{
  pkgs,
  self,
  home-manager,
}:

let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) isDarwin;

  homeDirectory = if isDarwin then "/Users/alice" else "/home/alice";

  mkHome =
    extraConfig:
    home-manager.lib.homeManagerConfiguration {
      inherit pkgs;
      modules = [
        self.homeModules.default
        {
          home = {
            username = "alice";
            inherit homeDirectory;
            stateVersion = "25.11";
          };
          manual.manpages.enable = false;
        }
        extraConfig
      ];
    };

  home = mkHome {
    programs.mmr = {
      enable = true;
      autoDiscoverProject = false;
      installSkill = true;
    };
    services.mmr-mcp = {
      enable = true;
      port = 8799;
      environment.RUST_LOG = "warn";
      environmentFile = "/run/secrets/mmr-mcp.env";
    };
  };

  # A bare `enable` writes no session variables.
  minimal = mkHome { programs.mmr.enable = true; };

  # Options from older releases still evaluate and are ignored.
  legacy = mkHome {
    programs.mmr = {
      enable = true;
      defaultSource = "codex";
      settings.summarize.model = "openai/gpt-5.5";
    };
  };

  inherit (home.config.services.mmr-mcp) package;
  exe = lib.getExe package;

  mmrVariables = cfg: lib.filterAttrs (name: _: lib.hasPrefix "MMR_" name) cfg.home.sessionVariables;
in
# home-manager traces warnings whenever a configuration is read, so the
# warning texts are covered by the NixOS module check, which shares them.
assert home.config.warnings == [ ];
assert minimal.config.warnings == [ ];
assert mmrVariables minimal.config == { };
assert mmrVariables home.config == { MMR_AUTO_DISCOVER_PROJECT = "0"; };
assert lib.length legacy.config.warnings == 2;
assert mmrVariables legacy.config == { };
assert !(legacy.config.xdg.configFile ? "mmr/config.json");
pkgs.runCommand "mmr-home-manager-module-test"
  {
    files = home.config.home-files;
    activation = home.activationPackage;
    sessionVars = home.config.home.sessionVariablesPackage;
  }
  (
    ''
      set -euo pipefail

      echo "the CLI is on the profile PATH"
      test -x "$activation/home-path/bin/mmr"
      "$activation/home-path/bin/mmr" --version

      echo "session variables carry the CLI defaults"
      grep -F 'MMR_AUTO_DISCOVER_PROJECT="0"' "$sessionVars/etc/profile.d/hm-session-vars.sh"

      echo "the agent skill matches what mmr skill install writes"
      skill_home="$(mktemp -d)"
      HOME="$skill_home" ${exe} skill install > /dev/null
      diff -r "$skill_home/.agents/skills/mmr" "$files/.agents/skills/mmr"
    ''
    + (
      if isDarwin then
        ''
          echo "the launchd agent runs the HTTP MCP server"
          plist="$activation/LaunchAgents/org.nix-community.home.mmr-mcp.plist"
          cat "$plist"
          wrapper="$(grep -o '/nix/store/[^<]*-mmr-mcp' "$plist")"
          grep -Fx '. /run/secrets/mmr-mcp.env' "$wrapper"
          grep -Fx 'exec ${exe} mcp --transport http --bind 127.0.0.1:8799 --path /mcp' "$wrapper"
          grep -F '<key>MMR_AUTO_DISCOVER_PROJECT</key>' "$plist"
          grep -F '<key>RUST_LOG</key>' "$plist"
        ''
      else
        ''
          echo "the systemd user service runs the HTTP MCP server"
          unit="$files/.config/systemd/user/mmr-mcp.service"
          cat "$unit"
          grep -Fx "ExecStart=${exe} mcp --transport http --bind 127.0.0.1:8799 --path /mcp" "$unit"
          grep -Fx 'Environment="MMR_AUTO_DISCOVER_PROJECT=0"' "$unit"
          grep -Fx 'Environment="RUST_LOG=warn"' "$unit"
          grep -Fx 'EnvironmentFile=/run/secrets/mmr-mcp.env' "$unit"
          grep -Fx 'KillSignal=SIGINT' "$unit"
          grep -Fx 'WantedBy=default.target' "$unit"
        ''
    )
    + ''
      touch "$out"
    ''
  )
