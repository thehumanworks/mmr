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
  inherit jsonFormat bindAddress;

  packageOption = mkOption {
    type = types.package;
    default = pkgs.callPackage ../package.nix { };
    defaultText = literalExpression "pkgs.callPackage \"\${mmr}/nix/package.nix\" { }";
    description = "The mmr package to use.";
  };

  programOptions =
    { settingsDescription }:
    {
      settings = mkOption {
        inherit (jsonFormat) type;
        default = { };
        example = literalExpression ''
          {
            summarize = {
              baseUrl = "https://openrouter.ai/api/v1";
              model = "openai/gpt-5.5";
              apiKeyEnv = "OPENROUTER_API_KEY";
            };
          }
        '';
        description = ''
          mmr configuration, written as {file}`config.json`. ${settingsDescription}

          mmr reads the `summarize` table: `baseUrl`, `model`, `apiKeyEnv`
          (the name of an environment variable that holds the key) and
          `apiKey`. Do not set `apiKey` here: everything in this option is
          copied to the world-readable Nix store. Use `apiKeyEnv` instead.
        '';
      };

      defaultSource = mkOption {
        type = types.nullOr (types.enum sources);
        default = null;
        example = "codex";
        description = ''
          Source that commands read when `--source` is omitted, exported as
          `MMR_DEFAULT_SOURCE`. `null` reads every source.
        '';
      };

      autoDiscoverProject = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Whether project-scoped commands such as `mmr list sessions` and
          `mmr read project` default to the project in the current directory.
          `false` exports `MMR_AUTO_DISCOVER_PROJECT=0`.
        '';
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
          OPENAI_BASE_URL = "https://openrouter.ai/api/v1";
          MMR_SUMMARISER_MODEL = "openai/gpt-5.5";
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
          `OPENAI_API_KEY` (the `mmr_summarize_*` tools) and `MORPHLLM_API_KEY`
          (the `mmr_compact_*` tools). ${environmentFileDescription}
        '';
      };
    };

  # Environment variables that carry the `programs.mmr` defaults.
  cliEnvironment =
    cfg:
    lib.optionalAttrs (cfg.defaultSource != null) { MMR_DEFAULT_SOURCE = cfg.defaultSource; }
    // lib.optionalAttrs (!cfg.autoDiscoverProject) { MMR_AUTO_DISCOVER_PROJECT = "0"; };

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

  settingsWarnings =
    prefix: settings:
    lib.optional (lib.attrByPath [ "summarize" "apiKey" ] null settings != null) ''
      ${prefix}.settings.summarize.apiKey copies your API key into the
      world-readable Nix store. Set ${prefix}.settings.summarize.apiKeyEnv to
      the name of an environment variable that holds the key instead.
    '';

  mcpWarnings =
    prefix: mcp:
    lib.optional (!(lib.hasPrefix "127." mcp.host || mcp.host == "::1")) ''
      ${prefix}.host is "${mcp.host}", which is not a loopback address. The
      mmr MCP server has no authentication, so anyone who can reach
      ${bindAddress mcp} can read your coding-session history.
    '';
}
