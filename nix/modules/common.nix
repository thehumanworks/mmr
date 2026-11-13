# Option declarations and helpers shared by the home-manager and NixOS modules.
{ lib, pkgs }:

let
  inherit (lib) literalExpression mkOption types;

  jsonFormat = pkgs.formats.json { };

  sources = [
    "claude"
    "codex"
    "cursor"
    "grok"
    "pi"
  ];

  bindAddress =
    mcp:
    if lib.hasInfix ":" mcp.host then
      "[${mcp.host}]:${toString mcp.port}"
    else
      "${mcp.host}:${toString mcp.port}";
in
{
  inherit bindAddress;

  packageOption = mkOption {
    type = types.package;
    default = pkgs.callPackage ../package.nix { };
    defaultText = literalExpression "pkgs.callPackage \"\${mmr}/nix/package.nix\" { }";
    description = "The mmr package to use.";
  };

  programOptions = {
    autoDiscoverProject = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Whether project-scoped commands such as `mmr list sessions` and
        `mmr read project` default to the project in the current directory.
        `false` exports `MMR_AUTO_DISCOVER_PROJECT=0`.
      '';
    };

    # No longer used. Still declared so that existing configurations
    # evaluate; `removedWarnings` says they are ignored.
    settings = mkOption {
      inherit (jsonFormat) type;
      default = { };
      visible = false;
      description = "Ignored. mmr no longer reads a config.json file.";
    };

    defaultSource = mkOption {
      type = types.nullOr types.str;
      default = null;
      visible = false;
      description = "Ignored. mmr reads every harness unless a command passes `--source`.";
    };
  };

  mcpOptions =
    {
      defaultPackage,
      defaultPackageText,
      environmentFileDescription,
    }:
    {
      enable = lib.mkEnableOption "the mmr MCP server over streamable HTTP" // {
        description = ''
          Whether to run `mmr mcp --transport http` as a background service,
          so MCP clients can connect to `http://<host>:<port><path>`. The
          server has no authentication and serves your coding-session history
          to anyone who can reach it.
        '';
      };

      package = mkOption {
        type = types.package;
        default = defaultPackage;
        defaultText = literalExpression defaultPackageText;
        description = "The mmr package that runs the server.";
      };

      host = mkOption {
        type = types.str;
        default = "127.0.0.1";
        example = "::1";
        description = ''
          IP address the server listens on. Keep it on loopback unless
          something else controls who can connect.
        '';
      };

      port = mkOption {
        type = types.port;
        default = 8765;
        description = "TCP port the server listens on.";
      };

      path = mkOption {
        type = types.strMatching "/[^[:space:]%]*";
        default = "/mcp";
        description = "HTTP path the MCP endpoint is mounted at.";
      };

      environment = mkOption {
        type = types.attrsOf types.str;
        default = { };
        example = {
          MMR_AUTO_DISCOVER_PROJECT = "0";
        };
        description = ''
          Extra environment variables for the server. These are stored in the
          world-readable Nix store, so put secrets in `environmentFile`.
        '';
      };

      environmentFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/run/secrets/mmr-mcp.env";
        description = ''
          File of `NAME=value` lines loaded when the server starts. Use it for
          values that must stay out of the Nix store.
          ${environmentFileDescription}
        '';
      };
    };

  # Environment variables that carry the `programs.mmr` defaults.
  cliEnvironment =
    cfg: lib.optionalAttrs (!cfg.autoDiscoverProject) { MMR_AUTO_DISCOVER_PROJECT = "0"; };

  mcpCommand = mcp: [
    (lib.getExe mcp.package)
    "mcp"
    "--transport"
    "http"
    "--bind"
    (bindAddress mcp)
    "--path"
    mcp.path
  ];

  removedWarnings =
    prefix: cfg:
    lib.optional (cfg.settings != { }) ''
      ${prefix}.settings is ignored and can be removed. mmr no longer reads
      config.json, because the model-backed summarize and compact commands
      were removed.
    ''
    ++ lib.optional (cfg.defaultSource != null) ''
      ${prefix}.defaultSource is ignored and can be removed. mmr reads every
      harness unless a command passes --source.
    '';

  mcpWarnings =
    prefix: mcp:
    lib.optional (!(lib.hasPrefix "127." mcp.host || mcp.host == "::1")) ''
      ${prefix}.host is "${mcp.host}", which is not a loopback address. The
      mmr MCP server has no authentication, so anyone who can reach
      ${bindAddress mcp} can read your coding-session history.
    '';
}
