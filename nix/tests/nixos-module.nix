# Evaluates NixOS systems that set the mmr options and checks the session
# variables, warnings and systemd unit, without booting anything.
{
  pkgs,
  self,
  nixpkgs,
}:

let
  inherit (pkgs) lib;

  mkSystem =
    extraConfig:
    nixpkgs.lib.nixosSystem {
      modules = [
        self.nixosModules.default
        {
          nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform.system;
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "none";
            fsType = "tmpfs";
          };
          system.stateVersion = "25.11";
          users.users.alice.isNormalUser = true;
        }
        extraConfig
      ];
    };

  system = mkSystem {
    programs.mmr = {
      enable = true;
      autoDiscoverProject = false;
    };
    services.mmr-mcp = {
      enable = true;
      user = "alice";
      host = "::1";
      port = 9876;
      path = "/memory/mcp";
      environment.RUST_LOG = "warn";
      environmentFile = "/run/secrets/mmr-mcp.env";
    };
  };

  # Options from older releases still evaluate, are ignored, and warn. A
  # server on a public address warns too.
  legacy = mkSystem {
    programs.mmr = {
      enable = true;
      defaultSource = "claude";
      settings.summarize.apiKey = "sk-test";
    };
    services.mmr-mcp = {
      enable = true;
      user = "alice";
      host = "0.0.0.0";
    };
  };

  # The server alone does not install the CLI or export its defaults.
  serviceOnly = mkSystem {
    services.mmr-mcp = {
      enable = true;
      user = "alice";
    };
  };

  inherit (system) config;
  exe = lib.getExe config.services.mmr-mcp.package;
  hasMmr = packages: lib.any (p: (p.pname or "") == "mmr") packages;
  warns = needle: lib.any (lib.hasInfix needle) legacy.config.warnings;
in
assert config.warnings == [ ];
assert lib.length legacy.config.warnings == 3;
assert warns "programs.mmr.settings is ignored";
assert warns "programs.mmr.defaultSource is ignored";
assert warns "services.mmr-mcp.host";
assert !(legacy.config.environment.sessionVariables ? MMR_DEFAULT_SOURCE);
assert !(legacy.config.environment.sessionVariables ? MMR_CONFIG_FILE);
assert !(legacy.config.environment.etc ? "mmr/config.json");
assert !(legacy.config.systemd.services.mmr-mcp.environment ? MMR_DEFAULT_SOURCE);
assert hasMmr config.environment.systemPackages;
assert !(hasMmr serviceOnly.config.environment.systemPackages);
assert config.environment.sessionVariables.MMR_AUTO_DISCOVER_PROJECT == "0";
assert !(serviceOnly.config.systemd.services.mmr-mcp.environment ? MMR_AUTO_DISCOVER_PROJECT);
pkgs.runCommand "mmr-nixos-module-test"
  {
    unitDir = config.systemd.units."mmr-mcp.service".unit;
  }
  ''
    set -euo pipefail

    echo "mmr-mcp.service runs as the named user with the CLI defaults"
    unit="$unitDir/mmr-mcp.service"
    cat "$unit"
    grep -Fx 'ExecStart="${exe}" "mcp" "--transport" "http" "--bind" "[::1]:9876" "--path" "/memory/mcp"' "$unit"
    grep -Fx 'User=alice' "$unit"
    grep -Fx 'Environment="MMR_AUTO_DISCOVER_PROJECT=0"' "$unit"
    grep -Fx 'Environment="RUST_LOG=warn"' "$unit"
    grep -Fx 'EnvironmentFile=/run/secrets/mmr-mcp.env' "$unit"
    grep -Fx 'KillSignal=SIGINT' "$unit"
    grep -Fx 'ProtectSystem=full' "$unit"

    touch "$out"
  ''
