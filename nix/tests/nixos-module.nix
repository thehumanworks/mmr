# Evaluates a NixOS system with every mmr option set and checks the generated
# /etc entry, session variables and systemd unit, without booting anything.
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
      defaultSource = "claude";
      autoDiscoverProject = false;
      settings.summarize = {
        baseUrl = "https://openrouter.ai/api/v1";
        apiKeyEnv = "OPENROUTER_API_KEY";
      };
    };
    services.mmr-mcp = {
      enable = true;
      user = "alice";
      host = "::1";
      port = 9876;
      path = "/memory/mcp";
      environment.MMR_COMPACT_MODEL = "morph-compactor";
      environmentFile = "/run/secrets/mmr-mcp.env";
    };
  };

  risky = mkSystem {
    programs.mmr = {
      enable = true;
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
in
assert config.warnings == [ ];
assert lib.length risky.config.warnings == 2;
assert hasMmr config.environment.systemPackages;
assert !(hasMmr serviceOnly.config.environment.systemPackages);
assert config.environment.sessionVariables.MMR_DEFAULT_SOURCE == "claude";
assert config.environment.sessionVariables.MMR_AUTO_DISCOVER_PROJECT == "0";
assert config.environment.sessionVariables.MMR_CONFIG_FILE == "/etc/mmr/config.json";
assert !(serviceOnly.config.environment.sessionVariables ? MMR_CONFIG_FILE);
assert !(serviceOnly.config.systemd.services.mmr-mcp.environment ? MMR_DEFAULT_SOURCE);
pkgs.runCommand "mmr-nixos-module-test"
  {
    nativeBuildInputs = [ pkgs.jq ];
    configFile = config.environment.etc."mmr/config.json".source;
    unitDir = config.systemd.units."mmr-mcp.service".unit;
  }
  ''
    set -euo pipefail

    echo "/etc/mmr/config.json holds the summarize settings"
    jq -e '.summarize == {
      apiKeyEnv: "OPENROUTER_API_KEY",
      baseUrl: "https://openrouter.ai/api/v1"
    }' "$configFile"

    echo "mmr-mcp.service runs as the named user with the CLI defaults"
    unit="$unitDir/mmr-mcp.service"
    cat "$unit"
    grep -Fx 'ExecStart="${exe}" "mcp" "--transport" "http" "--bind" "[::1]:9876" "--path" "/memory/mcp"' "$unit"
    grep -Fx 'User=alice' "$unit"
    grep -Fx 'Environment="MMR_DEFAULT_SOURCE=claude"' "$unit"
    grep -Fx 'Environment="MMR_AUTO_DISCOVER_PROJECT=0"' "$unit"
    grep -Fx 'Environment="MMR_CONFIG_FILE=/etc/mmr/config.json"' "$unit"
    grep -Fx 'Environment="MMR_COMPACT_MODEL=morph-compactor"' "$unit"
    grep -Fx 'EnvironmentFile=/run/secrets/mmr-mcp.env' "$unit"
    grep -Fx 'KillSignal=SIGINT' "$unit"
    grep -Fx 'ProtectSystem=full' "$unit"

    touch "$out"
  ''
