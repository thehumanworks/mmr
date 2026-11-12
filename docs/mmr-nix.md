# Install mmr with Nix

This repository is a Nix flake. It gives you three ways to install `mmr`:

1. **The flake itself**: `nix run`, `nix profile`, the `packages` output, or the
   overlay.
2. **home-manager**: `programs.mmr` for the CLI and its config file, and
   `services.mmr-mcp` for a per-user MCP server.
3. **NixOS**: `programs.mmr` for a system-wide CLI, and `services.mmr-mcp` for
   an MCP server that runs as one user.

The flake builds for `x86_64-linux`, `aarch64-linux` and `aarch64-darwin`. The
package runs the full Rust test suite while it builds.

## Run or install from the flake

```bash
nix run github:thehumanworks/mmr -- --help
nix profile add github:thehumanworks/mmr      # `nix profile install` on older Nix
```

While the repository is private, Nix needs GitHub credentials to fetch it.
Either add `access-tokens = github.com=<token>` to `~/.config/nix/nix.conf`,
or use the SSH form `git+ssh://git@github.com/thehumanworks/mmr` wherever these
examples say `github:thehumanworks/mmr`.

Pin a tag or commit for a reproducible install:

```bash
nix profile add github:thehumanworks/mmr/<tag-or-commit>
```

To use the package in your own flake, add the input:

```nix
{
  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz";
    mmr = {
      url = "github:thehumanworks/mmr";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
}
```

Then use `mmr.packages.${system}.default`, or apply the overlay and use
`pkgs.mmr`:

```nix
nixpkgs.overlays = [ inputs.mmr.overlays.default ];
environment.systemPackages = [ pkgs.mmr ];   # or home.packages
```

The flake also has a `home-manager` input. Only its own checks use it. If your
flake already has home-manager, add `inputs.home-manager.follows =
"home-manager";` to the `mmr` input so your lock file keeps one copy.

### What the package contains

- `bin/mmr`, wrapped so `git` (project root detection) and `ssh`/`scp` (remote
  peers) are on its `PATH`. They are appended, so your own `git` and `ssh` win.
- `share/mmr/skills/mmr`, the agent skill that `mmr skill install` writes.

## home-manager

```nix
{ config, inputs, ... }:
{
  imports = [ inputs.mmr.homeModules.default ];

  programs.mmr = {
    enable = true;
    defaultSource = "codex";   # optional; omit to read every source
    installSkill = true;       # links ~/.agents/skills/mmr
    settings.summarize = {
      baseUrl = "https://openrouter.ai/api/v1";
      model = "openai/gpt-5.5";
      apiKeyEnv = "OPENROUTER_API_KEY";
    };
  };

  # Optional: serve MCP over HTTP at http://127.0.0.1:8765/mcp.
  services.mmr-mcp = {
    enable = true;
    environmentFile = "${config.home.homeDirectory}/.config/mmr/mcp.env";
  };
}
```

On Linux, `services.mmr-mcp` is a systemd user service. Check it with
`systemctl --user status mmr-mcp`. On macOS it is a launchd agent labelled
`org.nix-community.home.mmr-mcp`, with logs in `~/Library/Logs/mmr-mcp.log`.

Both modules build mmr with your own `pkgs` by default, so your nixpkgs must
have Rust 1.88 or newer (NixOS 25.11 or later). On an older nixpkgs, use the
flake's own build instead:

```nix
programs.mmr.package = inputs.mmr.packages.${pkgs.stdenv.hostPlatform.system}.default;
```

### `programs.mmr` options

| Option | Default | Effect |
| --- | --- | --- |
| `enable` | `false` | Adds `mmr` to `home.packages`. |
| `package` | built from this flake with your `pkgs` | The mmr package to install. |
| `settings` | `{ }` | Written to `~/.config/mmr/config.json` (under `$XDG_CONFIG_HOME`). Nothing is written while it is empty. |
| `defaultSource` | `null` | Sets `MMR_DEFAULT_SOURCE` (`claude`, `codex`, `cursor`, `grok` or `pi`). |
| `autoDiscoverProject` | `true` | `false` sets `MMR_AUTO_DISCOVER_PROJECT=0`. |
| `installSkill` | `false` | Links the bundled agent skill into `~/.agents/skills/mmr`. |

### `services.mmr-mcp` options (home-manager and NixOS)

| Option | Default | Effect |
| --- | --- | --- |
| `enable` | `false` | Runs `mmr mcp --transport http` in the background. |
| `package` | `programs.mmr.package` | The mmr package that runs the server. |
| `host` | `"127.0.0.1"` | Listen address. A non-loopback address raises a warning. |
| `port` | `8765` | Listen port. |
| `path` | `"/mcp"` | HTTP path of the MCP endpoint. |
| `environment` | `{ }` | Extra environment variables. They are stored in the Nix store, so no secrets. |
| `environmentFile` | `null` | `NAME=value` file read at start, for `OPENAI_API_KEY` and `MORPHLLM_API_KEY`. |
| `user` | (NixOS only, required) | Account whose history the server serves. |

When `programs.mmr` is also enabled, the server gets the same
`MMR_DEFAULT_SOURCE` and `MMR_AUTO_DISCOVER_PROJECT` values as your shell.

## NixOS

```nix
{ inputs, ... }:
{
  imports = [ inputs.mmr.nixosModules.default ];

  programs.mmr.enable = true;

  services.mmr-mcp = {
    enable = true;
    user = "alice";
    environmentFile = "/run/secrets/mmr-mcp.env";
  };
}
```

`programs.mmr` takes `enable`, `package`, `settings`, `defaultSource` and
`autoDiscoverProject`, with the same meaning as above, applied to every user.
One difference: a non-empty `settings` is written to `/etc/mmr/config.json`
and `MMR_CONFIG_FILE` points at it, so it replaces each user's own
`~/.config/mmr/config.json`. Use the home-manager module for per-user
settings.

`services.mmr-mcp` is a system service, `mmr-mcp.service`, that runs as
`user` with that user's home directory. It reads the user's history and
`~/.config/mmr/config.json`, and keeps its store in `~/.local/share/mmr`. The
unit is sandboxed (read-only system directories, no capabilities, no new
privileges) but leaves the home directory writable, because mmr needs it.

## Connect an MCP client

Point any streamable-HTTP MCP client at `http://127.0.0.1:8765/mcp`. For
example, with Claude Code:

```bash
claude mcp add --transport http mmr http://127.0.0.1:8765/mcp
```

Clients that launch a stdio server do not need the service at all: configure
them to run `mmr mcp --transport stdio`.

## Security notes

- The MCP server has no authentication. Anyone who can connect can read your
  coding-session history. Keep `host` on loopback. The modules never open the
  firewall, and they warn when `host` is not a loopback address.
- Everything in `settings` and `environment` is copied to the world-readable
  Nix store. Put API keys in an environment variable named by
  `settings.summarize.apiKeyEnv`, or in `environmentFile`. The modules warn if
  `settings.summarize.apiKey` is set.

## Develop with Nix

```bash
nix develop                     # cargo, rustc, clippy, rustfmt, rust-analyzer, just, python3
nix build                       # build and test the package into ./result
nix flake check                 # package, module and NixOS VM checks for this system
nix fmt                         # format the Nix files
```

The `nixos-vm` check boots a virtual machine, so it needs a Linux builder with
KVM. The package reads `Cargo.lock` directly, so dependency updates need no
hash changes. `nix flake update` moves nixpkgs and home-manager forward.
