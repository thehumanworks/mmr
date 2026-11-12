{
  lib,
  stdenv,
  rustPlatform,
  makeBinaryWrapper,
  pkg-config,
  cacert,
  versionCheckHook,
  gitMinimal,
  openssh,
  zsh,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "mmr";
  inherit (cargoToml.package) version;

  # Only the files cargo needs. `src/cli.rs` embeds the bundled agent skill
  # from `.agents/skills/mmr` with `include_str!`, and the test suite runs in
  # the check phase.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../tests
      ../.agents/skills/mmr
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    makeBinaryWrapper
    pkg-config
  ];

  # `git` resolves project roots; `zsh` runs a paging contract test; the HTTP
  # client loads system CA certificates even when it only talks to localhost.
  nativeCheckInputs = [
    cacert
    gitMinimal
    zsh
  ];

  # The MCP and teleport contract tests bind loopback ports.
  __darwinAllowLocalNetworking = true;

  # `cursor_import_cli_contract_is_implemented` expects its temporary project
  # path to start with a prefix that mmr redacts as a local path (/tmp, /var,
  # /private, /home or /Users). The Linux sandbox sets TMPDIR to /build, so
  # move it into the sandbox's own /tmp.
  preCheck = lib.optionalString stdenv.buildPlatform.isLinux ''
    export TMPDIR="$(mktemp -d /tmp/mmr-check.XXXXXX)"
  '';

  postInstall =
    # Ship the same skill files that `mmr skill install` writes, so the
    # home-manager module can link them without running the binary.
    lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
      skill_home="$(mktemp -d)"
      HOME="$skill_home" "$out/bin/mmr" skill install > /dev/null
      mkdir -p "$out/share/mmr/skills"
      cp -r "$skill_home/.agents/skills/mmr" "$out/share/mmr/skills/mmr"
      rm -rf "$skill_home"
    ''
    # mmr runs `git` to find project roots and `ssh`/`scp` for remote peers.
    # Append them to PATH so a user's own git and ssh still win.
    + ''
      wrapProgram "$out/bin/mmr" \
        --suffix PATH : ${
          lib.makeBinPath [
            gitMinimal
            openssh
          ]
        }
    '';

  nativeInstallCheckInputs = [ versionCheckHook ];
  doInstallCheck = true;

  meta = {
    description = "Browse local AI coding-session history from Claude Code, Codex, Cursor, Grok and Pi";
    homepage = "https://github.com/thehumanworks/mmr";
    mainProgram = "mmr";
    platforms = lib.platforms.unix;
  };
}
