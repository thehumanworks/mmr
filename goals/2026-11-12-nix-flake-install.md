---
goal_id: "2026-11-12-nix-flake-install"
title: "Install mmr through Nix flakes, home-manager and NixOS"
description: "Ship a flake with package/app/overlay outputs plus home-manager and NixOS modules for the mmr CLI and its HTTP MCP server."
date: "2026-11-12"
status: "done"
confidence_floor: 85
created: "2026-11-12"
updated: "2026-11-12"
---

# Goal

Nix users can install `mmr` three ways from this repository, with no Cargo
toolchain of their own:

1. **Flake** — `nix run github:thehumanworks/mmr`, `nix profile install`, or the
   `packages`/`overlays` outputs in their own flake.
2. **home-manager** — `programs.mmr` installs the CLI and writes
   `~/.config/mmr/config.json`; `services.mmr-mcp` runs the HTTP MCP server as a
   per-user systemd service (Linux) or launchd agent (macOS).
3. **NixOS** — `programs.mmr` installs the CLI system-wide; `services.mmr-mcp`
   runs the HTTP MCP server as a hardened system service for one named user.

The user request ("Create a full nix integration for MMR, letting users install
'mmr' via nix flakes, home-manager or nixos module") is the approval to execute.

## Scope and invariants

- Add `flake.nix`, `flake.lock`, `nix/package.nix`, `nix/modules/*.nix`,
  `nix/tests/*.nix`, Nix install docs, and a README section.
- Module options mirror what mmr reads today: `config.json` (`summarize.*`),
  `MMR_DEFAULT_SOURCE`, `MMR_AUTO_DISCOVER_PROJECT`, and the `mmr mcp --transport
  http --bind --path` flags. No CLI contract changes (one Rust bug fix was
  added; see Decisions).
- Secrets never land in the Nix store: modules steer API keys to `apiKeyEnv` or
  an `environmentFile`, and warn if `summarize.apiKey` is set inline.
- The HTTP MCP server has no authentication, so modules warn when it binds to a
  non-loopback address and never open the firewall.
- The package runs the existing Rust test suite in the Nix sandbox.

## Definition of Done

- [x] DoD-1 — `nix build .#mmr` succeeds with tests enabled and the result runs
  `mmr --version` and `mmr skill load`; verify by `nix build` + run.
- [x] DoD-2 — `nix run . -- --help` works; verify by command output.
- [x] DoD-3 — home-manager module evaluates and builds an activation package
  with the config file, session variables, skill files and user service; verify
  by `nix build .#checks.<system>.home-manager`.
- [x] DoD-4 — NixOS module evaluates and produces the expected `/etc` entries
  and `mmr-mcp.service`; a NixOS VM test starts the service and answers an MCP
  `initialize` request where the builder supports VM tests; verify by
  `nix build .#checks.<system>.nixos-*`.
- [x] DoD-5 — `nix flake check` passes on x86_64-linux; Darwin derivations
  evaluate; verify by command output.
- [x] DoD-6 — Cargo verification loop still passes (fmt check, tests, clippy,
  release build); verify by the recorded commands.
- [x] DoD-7 — README and `docs/mmr-nix.md` document all three install paths and
  every module option; verify by review.

## Tasks

### T1 · Package and flake outputs · [x]

`nix/package.nix` (buildRustPackage, filtered source, git/ssh runtime PATH,
bundled skill under `share/`), flake `packages`, `apps`, `overlays`,
`devShells`, `formatter`, `checks`.

### T2 · home-manager module · [x]

`programs.mmr` (+ `settings`, `defaultSource`, `autoDiscoverProject`,
`installSkill`) and `services.mmr-mcp` (systemd user unit / launchd agent).

### T3 · NixOS module · [x]

`programs.mmr` (system package, environment defaults, optional global config)
and `services.mmr-mcp` (system unit running as a named user).

### T4 · Tests, docs, PR · [x]

Eval/VM checks, docs, full verification, PR.

## Exit Conditions

- DONE — all checks pass.
- BLOCKED-DEP — the build environment cannot fetch inputs or run checks.
- SCOPE-CHANGE — mmr needs Rust changes to be packaged.

## Evidence

- 2026-11-12 — Clean checkout of `628200c` on `claude/project-thread-v3mgry`.
  Read AGENTS.md, Cargo.toml, `src/config.rs`, `src/mcp.rs`, `src/store.rs`.
  mmr reads `MMR_CONFIG_FILE` or `$XDG_CONFIG_HOME/mmr/config.json`, stores
  data in `$XDG_DATA_HOME/mmr/mmr.db`, and shells out to `git`, `ssh`, `scp`
  and optionally `tailscale`.
- 2026-11-12 — `nix build .#mmr -L` passed on x86_64-linux: 365 tests ran in
  the sandbox (158 lib, 4 main, 139 cli_contract, 12 mcp, 47 memory_fabric,
  5 portable_bundle; 4 benchmarks ignored) and `versionCheckHook` found 0.2.0.
  The output has `bin/mmr` (wrapped) and `share/mmr/skills/mmr`.
- 2026-11-12 — `nix run . -- --help` and `nix run . -- skill load` printed the
  CLI help and the bundled skill.
- 2026-11-12 — `nix build .#checks.x86_64-linux.home-manager` passed: config
  JSON, session variables, skill files identical to `mmr skill install`,
  systemd user unit with ExecStart/Environment/EnvironmentFile/KillSignal.
- 2026-11-12 — `nix build .#checks.x86_64-linux.nixos-module` passed:
  `/etc/mmr/config.json`, session variables, `mmr-mcp.service` with `User=`,
  sandboxing, IPv6 bind; the two-warning risky config and service-only config
  assertions held.
- 2026-11-12 — `nix build .#checks.x86_64-linux.nixos-vm` passed under QEMU
  software emulation (no KVM here; run with `--option system-features
  "nixos-test benchmark big-parallel uid-range kvm"`): CLI on PATH,
  `MMR_DEFAULT_SOURCE` exported, `mmr list projects` as alice found the seeded
  Codex project, `mmr-mcp.service` ran as alice, answered MCP `initialize` and
  `tools/call mmr_list_projects`, and stopped without failing.
- 2026-11-12 — `nix flake check --no-build --all-systems` passed for
  x86_64-linux, aarch64-linux and aarch64-darwin with no warnings. The Darwin
  launchd agent definition was evaluated separately (ProgramArguments wrapper,
  KeepAlive, log paths). Darwin derivations were not built: no macOS builder
  with Nix was available.
- 2026-11-12 — Cargo loop in `nix develop`: `cargo fmt --all -- --check`,
  `cargo test --locked` (365 passed), `cargo test --locked --test
  cli_benchmark -- --ignored --nocapture` (4 passed), `cargo clippy --locked
  --all-targets --all-features -- -D warnings`, `cargo build --locked
  --release` all passed. `nix fmt` changed nothing; `git diff --check` clean.

## Decisions

- 2026-11-12 — nixpkgs input uses the `channels.nixos.org/nixos-unstable`
  tarball, which nixpkgs and home-manager now recommend over `github:` and which
  this build environment can reach. Locked to `d4f25a3e` (2026-11-11).
- 2026-11-12 — home-manager is an input only for `checks`. Its lock entry
  (`82ce401a`) was written from a `git archive` of that commit, because this
  environment cannot download GitHub tarballs. GitHub tarballs are `git archive`
  output and home-manager has no `.gitattributes`, so the NAR hash matches.
- 2026-11-12 — Module `package` defaults build mmr with the user's `pkgs`
  (`callPackage ./nix/package.nix`), not the flake's pinned nixpkgs. This keeps
  one nixpkgs in the closure and works on any system. The code already uses
  let-chains, so it needs Rust >= 1.88 (nixpkgs 25.11+); docs show how to use
  the flake's own build on older nixpkgs.
- 2026-11-12 — The package appends `gitMinimal` and `openssh` to mmr's PATH so
  services and minimal systems can resolve project roots and reach peers;
  user-installed tools still win. `tailscale` stays optional.
- 2026-11-12 — `services.mmr-mcp` is a system service with `User=` on NixOS
  (explicit owner, starts at boot, no per-login port clashes) and a user
  service/launchd agent in home-manager. Home stays writable because mmr keeps
  its store in `~/.local/share/mmr`; other sandboxing is on.
- 2026-11-12 — NixOS `programs.mmr.settings` sets `MMR_CONFIG_FILE`, which
  overrides per-user config. Documented; home-manager is the per-user path.
- 2026-11-12 — Scope addition (one Rust fix, separate commit). The sandboxed
  test run exposed a race in `src/peer.rs::run_ssh_command`: when a peer exits
  before reading its request, writing the request fails with a broken pipe and
  mmr reported `peer_ssh_failed` instead of the `peer_mmr_unavailable` that
  `read_project_remote_remote_peer_subcommand_missing_is_structured` expects.
  mmr now ignores a broken pipe there and classifies from the peer's exit and
  stderr. A new unit test forces the race with a 1 MiB request; it fails on
  the old code and passes on the new. `src/teleport/send.rs::stream_apply`
  has the same pattern but no failing test; left as follow-up.
- 2026-11-12 — home-manager traces warnings on any config access, so the
  warning cases are asserted in the NixOS module check, which shares the same
  helpers in `nix/modules/common.nix`.

## Learnings

- 2026-11-12 — nixpkgs (26.11pre) `cargoSetupHook` now exports
  `LIBSQLITE3_SYS_USE_PKG_CONFIG=1`, which overrides rusqlite's `bundled`
  feature. The first build failed at link time with `cannot find -lsqlite3`;
  the package now adds `pkg-config` and `sqlite`.
- 2026-11-12 — reqwest's rustls platform verifier loads system CA
  certificates when a client is built, so four unit tests panicked in the
  certificate-free sandbox. `cacert` in `nativeCheckInputs` provides them.
- 2026-11-12 — `cursor_import_cli_contract_is_implemented` fails whenever
  TMPDIR is outside /tmp, /var, /private, /home and /Users (reproduced locally
  with `TMPDIR=/build cargo test`). `capture.rs::sanitize_local_path_segments`
  only redacts paths under those prefixes, so a project under /build, /mnt,
  /opt, /srv or /root keeps its absolute path in searchable event text. The
  package moves TMPDIR into the sandbox's /tmp for the check phase; the
  redaction gap itself is a product follow-up, not changed here.
