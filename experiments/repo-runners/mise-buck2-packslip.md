# Mise, packslip and Buck2 addendum

Update: the original Buck2 screening below is superseded by [executed Cargo/Make adapter tests](buck2/README.md). The limitation is supplied host toolchains and explicit rules, not inability to build these applications.

Checked 2026-11-15. Documentation/API verification, not new application build tests.

## Latest mise and the original command

GitHub's `/repos/jdx/mise/releases/latest` API returned **v2026.11.8**, published **2026-11-13T13:07:07Z**. The current official docs also identify v2026.11.8. Release: https://github.com/jdx/mise/releases/tag/v2026.11.8

`mise exec github:org/repo -- command` is an appropriate interface when the project publishes platform release assets. The GitHub backend selects/downloads release assets; it does not compile arbitrary Git source or use GitHub-generated source archives. Source: https://mise.jdx.dev/dev-tools/backends/github.html

There is an important narrower match for **mmr**: the **cargo backend supports Git source directly**. A candidate invocation is:

```sh
mise exec rust@<tested-rust-version> 'cargo:owner/mmr@rev:<commit>' -- mmr <arguments>
```

This command shape is documentation-derived and was not executed in this addendum. The cargo backend accepts GitHub owner/repo or a full Git URL. Git sources always use `cargo install --git`; version selectors support `tag:`, `branch:`, `rev:`; default/latest means default branch. `--locked` is the default. Mise installs configured Rust before Cargo tools, but a linker and native dependencies remain requirements. Thus this can remove a custom wrapper for the Rust application; it does not generalize to tny's Make/CPython build.

Verified against both current official docs and the documentation file in the exact latest tag:

- https://mise.jdx.dev/dev-tools/backends/cargo.html
- https://github.com/jdx/mise/blob/v2026.11.8/docs/dev-tools/backends/cargo.md

## Remote tasks

Mise can load a task script from Git:

```toml
[tasks.build]
file = "git::https://github.com/org/repo.git//scripts/build.sh?ref=<commit>"
```

This is remote-script execution, not automatic interpretation of a repository's build system. The script must define checkout/source handling, tool provisioning, build and run behavior as appropriate. It could host a generic reusable runner, but is not evidence that `github:` source-builds C++ projects. Remote task scripts are cached; `mise run --no-cache` refreshes them. Sources:

- https://mise.jdx.dev/tasks/toml-tasks.html
- https://mise.jdx.dev/cli/tasks/run.html

## packslip

Packslip is a signed release-manifest format and installer. It authenticates publisher identity, artifact digest, size and platform selection. Mise's packslip backend installs the described executables; it is not a general source recipe/compiler engine. The publisher must supply suitable artifacts, however those artifacts were built. GitHub Actions is one publishing route, not a reason compilation disappears. Building the packslip CLI itself with Cargo is separate from building applications distributed through it.

- https://mise.jdx.dev/dev-tools/backends/packslip.html
- https://packslip.dev/
- https://packslip.dev/docs/publishing/
- https://packslip.dev/docs/install/

## Buck2: capable, but not the requested native URL interface

Buck2 **can fetch GitHub source** using a Git external cell:

```ini
[cells]
  root = .
  application = application
[external_cells]
  application = git
[external_cell_application]
  git_origin = https://github.com/org/repo
  commit_hash = <40-character-commit>
```

The commit must be a SHA, not a branch name. Build/run addresses a BUCK target such as `buck2 run application//:app -- args`, not a bare Git URL. External cells are configured in the root project, cannot declare transitive external cells, and their own cells configuration is ignored. These limitations matter for a generic repository runner because a fetched project's complete toolchain graph does not automatically become the root project's graph.

Buck2 can provision hermetic compilers: official docs describe downloading tools with `http_archive` and exposing them through toolchain providers; the prelude includes a Zig-based C++ example. Demo toolchains commonly use tools already on PATH. Therefore it is wrong to claim Buck2 inherently needs preinstalled compilers, but creating a complete downloadable compiler/SDK/tool graph is integration work.

For mmr, Rust targets plus Cargo dependencies translated through Reindeer are a plausible route. For tny, either translate build targets or write a declared action around the existing Make/CPython build, including tool and archive inputs discussed in the Bazel assessment. A generic Git wrapper can provide the desired user interface after repository conventions and toolchain metadata exist. No completed Buck2 build proves those integrations here; its current status is capable in principle, not validated for these apps.

- Git external cells and limitations: https://buck2.build/docs/users/advanced/external_cells/
- Run target syntax: https://buck2.build/docs/users/commands/run/
- Downloaded versus PATH toolchains: https://buck2.build/docs/rule_authors/writing_toolchains/
- Reindeer use by Buck2: https://buck2.build/docs/about/bootstrapping/
- Reindeer Cargo graph translation: https://github.com/facebookincubator/reindeer/blob/main/docs/MANUAL.md

## Verdict adjustment

Mise itself already offers the source-Git workflow for Rust through `cargo:`, subject to native linker/dependency availability. Its `github:` and `packslip:` backends solve distribution of existing builds. Buck2 can support both applications with build/toolchain integration and a generic wrapper, but gives no automatic arbitrary-repository build interpretation. These distinctions should replace blanket claims that GitHub references are unsupported.

## Native mise Cargo test: verified on macOS arm64

Subsequently executed with downloaded mise **2026.11.8**:

```sh
mise exec 'cargo:thehumanworks/mmr@rev:daf2639dcd5768c6ad809b1e771b0b9029d0eb24' -- mmr --version
```

Result: exit 0, `mmr 0.2.0`. Fresh Git checkout and crates cache, source compilation and execution took **29.44 seconds**; immediate repeat took **0.0184 seconds**, also exit 0. The command itself uses no custom runner or project modifications. The Python harness only isolates directories, sets PATH, measures time and saves logs.

Scope: existing Rust **1.99.1** was selected by its actual toolchain bin path, with existing Apple system compiler/SDK tools. Mise/Cargo cache/data/config/state were fresh and confined to `alternatives/mise-cargo/`. This verifies source-build/run with the host compiler prerequisites present; it does not verify clean-machine Rust/SDK bootstrap or Linux. Only `--version` ran; no personal mmr data was accessed.

Initial harness attempt exposed only rustup's shim path while relocating CARGO_HOME and failed to locate cargo. Selecting the actual already-installed toolchain fixed it. During the successful build, Rust emitted a nonfatal warning that rust-objcopy could not load libLLVM.dylib for stripping debug info; compilation, installation and execution succeeded. No host toolchain repairs or global changes were made.

Evidence:
- `alternatives/mise-cargo/test.py`
- `alternatives/mise-cargo/result.json`
- `alternatives/mise-cargo/cold.log`
- `alternatives/mise-cargo/warm.log`
- `alternatives/mise-cargo/first-attempt-missing-cargo.log`
