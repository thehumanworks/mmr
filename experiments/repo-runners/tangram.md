# Tangram evaluation

Verdict: promising Nix-like toolchain/build/cache system, but not a verified solution for the requested direct GitHub runner workflow. Runtime testing was blocked by the official installation download returning HTTP 404. No project files were changed.

## Verified by network probe and source inspection

- Official installer https://tangram.dev/install.sh supports aarch64/x86_64 Linux and Darwin. It downloads https://github.com/tangramdotdev/tangram/releases/latest/download/tangram_aarch64-darwin.tar.gz on this host. That exact URL returned HTTP 404. GitHub API `/repos/tangramdotdev/tangram/releases/latest` also returned 404 and `/releases?per_page=5` returned `[]`.
- Current source cloned at `e80203a092aa2a2fab7f9123a67ac7b14a37d278`; packages at `d522ff1a63b7e2245d77aa0c400457836f67730e`.
- CLI `tg run REFERENCE -- ARGS` accepts a Tangram reference. Official https://www.tangram.dev/docs/references defines reference as ID, local path, or tag. Source `packages/clients/rust/src/reference.rs` defines ID/Path/Pointer/Specifier nodes; no Git transport. A tag that resembles `owner/repo` is a Tangram tag, not an automatic GitHub clone. Therefore do not advertise `tg run github:owner/repo`.
- Official https://www.tangram.dev/docs/packages requires a tangram.ts or tangram.js entrypoint. CLI supports `tg run .#export -- arguments`. A wrapper would need fetch source and invoke its recipe, or publisher must register Tangram package/tag.
- Toolchain provisioning is substantial: https://github.com/tangramdotdev/packages/blob/main/packages/std/sdk.tg.ts supplies compiler, linker, libc, basic utilities; defaults choose prebuilt/cache SDK builds. GNU toolchain rejected on Darwin; LLVM used there. https://github.com/tangramdotdev/packages/blob/main/packages/rust/tangram.ts downloads Rust components and composes SDKs, supports target triples.
- https://github.com/tangramdotdev/packages/blob/main/packages/autobuild/tangram.ts auto-detects Cargo.toml, CMakeLists.txt, executable configure/configure.ac, and several other language manifests. It has no plain Makefile detector. This is a possible generic helper for mmr; tny requires an explicit recipe. Neither was executed.
- Source/bootstrap dependencies still need binaries or compilation. `packages/std/bootstrap.tg.ts` downloads bootstrap tarballs from tangramdotdev/bootstrap releases. This does not require publishing *your application* as binaries or using GitHub Actions, but is not a build-free system.
- Source installer `nu scripts/install.nu --directory PATH` is available and invokes cargo build --all-features --release. Building Tangram itself would add a large Rust/native bootstrap prerequisite; not attempted in this bounded evaluation.

## Meaning for these repositories

For mmr, a Tangram recipe can use Rust cargo builder and managed SDK. For tny, a recipe must express Makefile targets, native dependencies and output path. Tangram would then perform local source builds and cache outputs on supported hosts. Native macOS and Linux support is present in source/installer; actual mmr/tny runtime success is unverified. Cross-compilation support does not prove that arbitrary tny dependency builds work across OS boundaries.

Tangram may be more capable than a simple environment manager for reproducible builds, but current accessible release distribution and missing direct GitHub reference support prevent recommending it as the tested easy runner. A generic Git-fetch wrapper remains required for the exact desired syntax unless the user adopts Tangram tags instead.

## Reproduction

```sh
curl -fsSL https://tangram.dev/install.sh
curl -fL https://github.com/tangramdotdev/tangram/releases/latest/download/tangram_aarch64-darwin.tar.gz
curl -fsSL 'https://api.github.com/repos/tangramdotdev/tangram/releases?per_page=5'
```

The first succeeded; second failed HTTP404; third returned an empty array. No Tangram executable was installed and no Linux task was started.
