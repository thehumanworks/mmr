# Additional repository runner candidates

Research date: 2026-11-15. This is documentation screening, not executable validation. No global configuration or project source was changed. Tangram and the practical Nix/Pixi/Bazel experiments are evaluated separately.

Requirement: supply a GitHub repository to one generic runner; fetch source and compiler/dependencies; build locally if needed; run natively on Linux and macOS; no application release binaries or GitHub Actions dependency.

## Findings

**No additional candidate examined is a stronger direct match than a configured Nix flake.** Xmake and Spack can perform local source builds with dependency management, but require package recipes and an additional fetch/run interface. Devenv now accepts GitHub references directly, but those references load environment configuration into the current directory rather than automatically building the source repository.

| Tool | What is supported | Gap for mmr + tny workflow |
| --- | --- | --- |
| devenv | `devenv --from 'github:org/repo?dir=environment' shell`; shell/test/up/build accept remote configuration. Native Linux/macOS through Nix. | Requires devenv configuration and an explicit source/build/run recipe. `--from` binds configuration to the current project; it is not by itself a generic remote application runner. Existing flakes are more direct. |
| Xmake/xrepo | Native Linux/macOS build manager; package recipes fetch Git sources; `xrepo env -b 'package-list' command` installs packages and runs command. Can download LLVM, Zig, muslcc toolchains. | A Git URL supplied to `xrepo add-repo` identifies a package recipe repository. Arbitrary source repositories still need recipes and checkout/run orchestration. Rust has a toolchain, but existing Cargo/Make projects would need integration work. |
| Spack | Native Linux/macOS source package manager, Git source recipes, dependency and compiler graph, build caches. Custom recipe repositories can be fetched from Git. | CLI installs package specs, not arbitrary Git source as an application. Requires package recipes and an execution wrapper; requires system prerequisites and macOS Command Line Tools. Can build more compilers after bootstrapping. |
| pkgx + dev | Downloads language/tool binaries on Linux/macOS; `dev` detects project metadata such as Cargo.toml. | Manages tools, not arbitrary application source fetch/build/run. Could support a generic wrapper, but adds no stronger packaging contract than Pixi here. |
| pkgo | Uses pkgx and contributed per-project setup/run scripts. Docs claim macOS/Linux/Windows, with some platforms untested. | Explicitly requires the user to clone the project; scripts live in pkgo's manifest repository. Not a generic GitHub runner. |
| Brioche | Reproducible source recipes, dependency graph and caching, simple TypeScript definitions. | Current supported sandbox backends and installation are Linux only; native macOS requirement fails. |
| Hermit | Consistent cross-platform development-tool environments. | Docs explicitly say binary-only packages and no library management. Does not supply this source-build workflow. |
| Flox | `flox activate -r owner/environment -- command` activates a remote environment with Nix packages. | Reference points to FloxHub, not arbitrary GitHub source; application source build still needs recipe/hooks. Adds a separate publication mechanism. |

These tools do not remove compilation from a first uncached source execution. They can move compiler setup and compilation behind one command. A source recipe remains necessary to define dependencies, output binary, and any special build rules.

## Primary sources

- Devenv remote configuration: https://devenv.sh/guides/out-of-tree-devenvs/ (`--from` added in 2.0; fetched full YAML configuration fixed in 2.4.1). Linux/macOS and Nix requirement: https://devenv.sh/
- Xrepo command/environment semantics: https://xmake.io/guide/package-management/xrepo-cli
- Xmake automatic compiler download: https://xmake.io/guide/project-configuration/toolchain-configuration
- Xmake source Git URLs in package recipes: https://xmake.io/api/description/package-dependencies.html
- Spack Git recipe repositories: https://spack.readthedocs.io/en/latest/repositories.html
- Spack startup/compiler discovery: https://spack.readthedocs.io/en/latest/getting_started.html
- Spack host prerequisites: https://spack.readthedocs.io/en/latest/installing_prerequisites.html
- pkgx supported platforms/tool ecosystem: https://github.com/pkgxdev/pkgx
- pkgo manual-clone and contributed-manifest design: https://github.com/pkgxdev/pkgo
- Brioche supported sandbox platforms: https://brioche.dev/docs/configuration/ and installation https://brioche.dev/docs/installation/
- Hermit binary-only/library exclusions: https://cashapp.github.io/hermit/faq/
- Flox remote environment command: https://github.com/flox/flox/blob/main/cli/flox/doc/flox-activate.md and examples https://flox.dev/environments/

## Recommendation

Keep the practical comparison focused on Nix, Tangram, and the reusable Pixi wrapper. Xmake is a reasonable optional fourth experiment if replacing or wrapping each application's build metadata is acceptable. Spack requires more setup without improving the requested invocation. Devenv is useful if the actual objective is sharing development environments; it does not improve the already-existing flake-based application runner for these repositories.
