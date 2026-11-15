# Bazel integration assessment for mmr and tny

Bazel can plausibly deliver this workflow for both projects after adding build definitions and a reusable Git wrapper. The observed direct-URL and missing-MODULE failures only show that the requested interface/configuration is absent today. They do not show that Bazel cannot fetch or build these repositories. A deeper build test is warranted before claiming build incompatibility or parity with the tested Pixi route.

Status: source inspection and current primary documentation only. No Bazel integration build was started; no application files were edited. This assessment does not establish successful Linux/macOS execution.

## mmr: modest production-build integration

Inspected `/Users/tomas/projects/mmr/Cargo.toml`, `src/`, and compile-time includes. It is one Rust package/binary, edition 2024, rust-version 1.85, with Cargo.lock and no project build.rs found. Dependencies include derive/procedural macros (clap, serde, rmcp, schemars), Tokio, Axum, and Rust libraries. A recent compiler must satisfy the *resolved dependency* requirements, not merely the package's declared minimum.

Concrete integration:

1. Add a pinned `.bazelversion`, `MODULE.bazel` with `rules_rust`, and an explicit downloadable Rust toolchain.
2. Use `crate.from_cargo(name="crates", cargo_lockfile="//:Cargo.lock", manifests=["//:Cargo.toml"])`. This reads Cargo dependency/feature metadata rather than maintaining an independent handwritten dependency list. Dependency generation may require annotations for individual transitive crates; that is a test question.
3. Add `rust_binary(name="mmr", crate_root="src/main.rs", srcs=glob(["src/**/*.rs"]), edition="2024", deps=all_crate_deps(normal=True), proc_macro_deps=all_crate_deps(proc_macro=True), ...)`.
4. Declare compile-time Markdown inputs, including hidden `.agents/skills/mmr/**` and `src/agent_instructions.md`, as `compile_data`; ordinary Rust source globs will not include these. Supply `CARGO_PKG_VERSION` through the rule's version support or rustc environment. The application uses that variable for CLI and protocol versions.
5. Run the target with argument forwarding and confirm it preserves caller working directory: mmr uses cwd to infer project scope. Its integration tests also expect Cargo's `CARGO_BIN_EXE_mmr`; production execution and full test migration are separate amounts of work.

The production BUILD definition should be compact. Full Cargo-test equivalence requires further test definitions and environment handling. No reason found in inspected source that rules out Bazel.

## tny: viable Make adapter, significant hermetic input/tool work

Inspected `/Users/tomas/projects/tny/Makefile` and `scripts/cpython_runtime.sh`. The Makefile already supports `PREFIX`, `DESTDIR`, `CC`, `CXX`, `BUILD`, `TNY_VERSION`, `CPYTHON_FETCH`, `CPYTHON_TARBALL`, and `CPYTHON_DEPS_DIR`; `install` builds release then installs bin/tny plus support files. These are useful existing seams: no full C/C++ BUILD-file rewrite is necessary.

Use `rules_foreign_cc.make`, source filegroup including first-party sources, vendored sources, scripts, Python package files, shell assets, licenses, and pinned dependency metadata. Configure install target and `out_binaries=["tny"]`; declare installed support directories where needed. Expose an executable target for `bazel run` (a foreign build's output is not automatically the same thing as a runnable target). Preserve the installed layout or supply runfiles-aware launching if ancillary files are required.

The substantial integration work is CPython packaging:

- Build script compiles pinned CPython and zlib, bzip2, xz, zstd, sqlite, libffi, OpenSSL. It then freezes standard library modules and embeds the runtime.
- Declare the eight source archives as repository downloads with their existing SHA256 pins. Stage them as action inputs and set `CPYTHON_FETCH=0`, rather than making network-dependent curl calls inside the compile action.
- Declare/provision GNU Make, Perl (OpenSSL configure), archive/compression tools and shell utilities. The script invokes `make`, `ar`, and `ranlib` by their plain command names. Passing a CC toolchain alone does not guarantee these resolve to the desired binaries; provide a controlled tool PATH or adapt the build script's tool overrides.
- Set a deterministic `TNY_VERSION`; source archives/external repositories need not contain Git metadata. Ensure Python source files used for embedded package generation and license inputs are included.
- CPython's configure/build executes a freshly built interpreter to freeze its standard library. Same-host Linux/macOS builds fit this design; cross-compiling from Linux to macOS is a different and harder problem, not required by the user's native-on-each-OS requirement.

Using the existing Makefile means Bazel initially sees much of the native build as one coarse action. It can cache that action, but does not automatically gain per-C-source Bazel incrementality. Rewriting the full build graph is optional and would be substantially more work.

## Compiler and platform provisioning

`rules_rust` can download Rust tools for supported hosts. `rules_foreign_cc` provides Make/CMake toolchain integration, but does not by itself provide a complete OS C/C++ SDK. `toolchains_llvm` supports downloadable LLVM distributions and registered C/C++ toolchains on Linux/macOS. Its documentation explicitly separates the compiler distribution from OS headers/libraries: an appropriate SDK/sysroot must still exist or be supplied.

Consequently, a working build on this Mac with Command Line Tools present does not prove a fresh Mac without Apple's SDK works. Likewise, a Linux host's libc/tool layout can affect an insufficiently declared build. These caveats also deserve equal treatment in the Pixi comparison. Downloaded compiler binaries are not the forbidden application release binaries; the application would still build on the user's machine.

## Generic wrapper and fair verdict

A reusable `repo-run bazel github:owner/repo@revision -- command...` can cache a checkout, invoke one declared conventional runnable target, and forward arguments. This can give the requested one-command source-build experience without GitHub runners. Repositories must contain Bazel definitions or an external integration repository must overlay them; a wrapper cannot infer arbitrary build requirements reliably.

Bazel itself supports Git repositories and BUILD overlays (`git_repository`/`new_git_repository`, or archive repositories) without requiring the upstream project to have MODULE.bazel. A `git_override` specifically selects a Bazel module, so failure on an unconfigured upstream is not the limit of Bazel's Git integration.

The wrapper would be equivalent to the Pixi prototype at the user-interface level once both projects' Bazel definitions are complete and tested. It is not equivalent merely by cloning and calling `bazel run`: Pixi consumes the supplied project task metadata; Bazel must receive its own metadata and toolchain definitions. Conversely, Pixi is also not an arbitrary-source inference engine. Both need declared build knowledge.

Recommendation: label Bazel **capable, integration not completed; more setup than Pixi for these existing Cargo/Make projects**. If a full Bazel comparison is required, first implement mmr's small target, then tny's Make adapter with declared source archives. Validate both on native Mac and Linux, cold and warm execution, argument forwarding, caller cwd, and offline repeat builds. Do not present the present URL/configuration failures as a failed application build.

## Primary references

- Cargo dependency generation and procedural macro examples: https://bazelbuild.github.io/rules_rust/crate_universe_bzlmod.html
- Rust toolchain registration: https://bazelbuild.github.io/rules_rust/ and https://bazelbuild.github.io/rules_rust/rust_bzlmod.html
- GNU Make adapter, install-prefix/output requirements, build_data: https://bazel-contrib.github.io/rules_foreign_cc/make.html
- Bzlmod build-tool provisioning examples: https://bazel-contrib.github.io/rules_foreign_cc/bzlmod_examples.html
- LLVM distributions and separate SDK/sysroot responsibilities: https://github.com/bazel-contrib/toolchains_llvm
- Bazel Git fetch and BUILD overlay implementation: https://github.com/bazelbuild/bazel/blob/master/tools/build_defs/repo/git.bzl
