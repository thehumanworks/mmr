# Tangram macOS investigation — 2026-11-15

Tangram **does run natively on macOS ARM64**, including a sandboxed build with its downloaded LLVM compiler and macOS SDK. Its standard `std.sdk` toolchain also compiled and ran a native C program. The earlier installer-404 result was an installation problem, not proof that macOS execution is unavailable.

This does **not** establish the user's full two-repository, Linux/macOS workflow. We did not build mmr or tny with Tangram, test Linux execution, test Intel Macs, or test older macOS releases. Tangram still needs a build recipe and does not natively interpret a GitHub repository URL as an instruction to clone, infer its build, and run a named command.

## Verified results

| Test | Result |
| --- | --- |
| Official CLI installer download | HTTP404; latest-release API404; releases list empty at check time |
| CLI built from official source | PASS, `tangram 0.0.0`, native ARM64 Mach-O |
| TypeScript recipe creates artifact | PASS |
| Recipe invokes host `/usr/bin/clang` | PASS; this first test alone proves only host-tool execution |
| Recipe downloads official LLVM+SDK and compiles native C | PASS; downloaded compiler, sandboxed child, exit0 |
| Run downloaded-compiler output | PASS: `Tangram managed native macOS build succeeded` |
| Repeat downloaded-compiler build | PASS; same artifact, approximately0.016s warm invocation |
| `std.sdk(await std.bootstrap.sdk.arg())` compiles native C | PASS after fixing isolated store discovery; sandboxed child, exit0 |
| `tg run` standard-SDK output | PASS: `Tangram std.sdk macOS build succeeded` |
| Direct `--checkout` standard-SDK output | FAIL: `cannot check out a dependency without a store path`; `tg run` works |

Host: macOS27.3 build26C5035r, ARM64. `otool -l` on the built CLI reports **minOS26.0**, SDK27.0, matching source `.cargo/config.toml` deployment target26.0. This build is not evidence of support for pre26 Macs. The installer/source describe both x86_64 and aarch64 Darwin/Linux, but only aarch64 Darwin was executed here.

## Exact source bootstrap

Pinned source:

- `tangramdotdev/tangram`: `e80203a092aa2a2fab7f9123a67ac7b14a37d278`
- `tangramdotdev/packages`: `d522ff1a63b7e2245d77aa0c400457836f67730e`

The existing machine supplied Git, rustup/Cargo launchers, Bun/bunx, and Apple's developer command tools for building Tangram itself. Rust1.100.0 was installed into isolated directories. No global Tangram installation or global configuration was changed. The final build used:

```sh
export RUSTUP_HOME="$ROOT/rustup" CARGO_HOME="$ROOT/cargo"
export RUSTUP_TOOLCHAIN=stable # resolved to Rust1.100.0 during this test
export CARGO_TARGET_DIR="$ROOT/target" BUN_INSTALL_CACHE_DIR="$ROOT/bun-cache"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=off
cargo build --locked --package tangram_cli --bin tangram \
  --no-default-features --features lmdb,sqlite,tls,quickjs,tsrs,python -j 6
```

Run in `$ROOT/source`. QuickJS is a supported runtime; this avoids unrelated V8/cloud integrations. Python enables parsing Python files during check-in of the packages repository; without it, importing std failed with `python feature is not enabled`. The first build without Python took5m47; adding Python took2m28. Because both builds report0.0.0, the existing isolated daemon needed an explicit stop before it used the rebuilt binary.

`proof-artifacts/reproduce.sh` captures these steps with Rust pinned to1.100.0. It has been syntax-checked; the entire script was not run again from an empty directory. Its component operations were executed manually. Use a new evaluation directory. The fixture is deliberately ARM64/macOS-specific, not a cross-platform runner implementation.

## Managed compiler and sandbox evidence

`proof-artifacts/managed-bootstrap/tangram.ts` downloads checksummed official bootstrap release `v2026.09.16` LLVM and SDK27 archives. It runs `{ artifact: toolchain, path: "bin/clang" }`, supplies the downloaded SDK as `-isysroot`, and uses only the downloaded toolchain's `bin` as its PATH. The generated program is native arm64, not a container or VM.

The successful compiler process is `pcs_0006gy08hendrpbce10r6dx048gw`. Its sanitized metadata in `managed-compiler-process.txt` reports `cacheable: true`, `host: aarch64-darwin`, `exit: 0`, and a sandbox ID. The standard-SDK compiler process `pcs_0006gy09q4dxsq936t25vj6xnmgm` also has an explicit sandbox ID and exit0; see `std-sdk-compiler-process.txt`. The macOS backend uses Seatbelt. These observations establish use of the sandbox path, not an independent security audit of all its policies.

`proof-artifacts/managed-native/tangram.ts#standardSdk` exercises the standard SDK using its bootstrap toolchain argument. It does not rebuild the full default LLVM toolchain from source. Standard SDK setup downloaded Rust and compiled its wrapper/linker/compiler helper proxies. Those Rust compilations were **toolchain helpers, not an mmr application build**. Application C compilation then used the managed compiler through `cc`.

The compiler and SDK are downloaded platform artifacts maintained by Tangram. Thus this removes the need for **your own application's** release artifacts and GitHub runners; it does not eliminate upstream prebuilt toolchains or the local application build itself.

## Problems diagnosed, rather than treated as platform failures

1. Official CLI release URLs404: source bootstrap succeeded.
2. A first probe represented the compiler as a template; process metadata showed an invalid executable. Using the documented artifact+path executable representation fixed this fixture error.
3. std imports checked in Python files; enabling the CLI Python parser fixed check-in.
4. The standard SDK wrapper exited111: `failed to find the store directory`. Its `std/packages/wrapper/include/manifest.h` searches `/.tangram/store`, `/opt/tangram/store`, or an ancestor's `.tangram/store`. Our custom `-d "$ROOT/state"` did not meet that layout. An isolated `$ROOT/.tangram -> state` symlink fixed discovery. `build --retry` reran the cached failure and succeeded. The reproduction script uses `$ROOT/.tangram` directly.
5. Direct checkout of the resulting wrapped artifact failed because dependency checkout lacked a store path. Running the artifact using `tg run` succeeded. Do not present the wrapper output as a demonstrated standalone portable binary.

The isolated test daemon was stopped after evidence collection. A normal command restarts it automatically; to rerun the verified artifact in the existing evaluation directory:

```sh
ROOT=/Users/tomas/repo-runner-evaluation/tangram
PATH="$ROOT/bin:$PATH" "$ROOT/bin/tg" -c "$ROOT/config.json" -d "$ROOT/state" \
  --no-remotes run fil_01rwjwt4fcfhvnjpw3d76ktfencnm5mknbkacfkvq06g9amya7t5f0
```

Keep the local `.tangram -> state` alias for this existing evaluation. Fresh reproduction uses `.tangram` as the state directory directly.

All raw logs remain in the evaluation directory. The small `proof-artifacts` directory contains sanitized evidence and recipes, with no binaries, caches, or local process capability tokens.

## Fit for the requested workflow

Tangram is a substantive candidate when a repository provides a Tangram recipe. It offers native sandboxed builds, content-addressed caching, and managed compiler/SDK packages. A first execution builds the application locally unless an appropriate build result is already cached; subsequent execution can reuse it.

Its native references are Tangram IDs, registry tags, or local paths. A literal GitHub URL is not a generic clone-and-build runner interface. To obtain `<runner> <github_repo> -- command`, a reusable checkout wrapper would need to clone/pin the source and invoke a recipe, or the project would need to publish through Tangram's registry. The packages repository has an `autobuild` package with Cargo/CMake/autotools detection, but this does not remove the need to resolve source, define dependencies and expose the resulting command. tny's plain Makefile is not automatically handled by those detectors.

**Verdict:** native macOS operation and managed native compilation are proven. Tangram is not yet demonstrated as a ready replacement for the tested generic runner on mmr and tny across Linux/macOS. Source-only CLI installation at this snapshot, minOS26 in this build, required recipes, and the observed directory/checkout issues make adoption more work than the simple command suggests.

Primary references: [official source](https://github.com/tangramdotdev/tangram), [reference syntax](https://www.tangram.dev/docs/references), [package entrypoints](https://www.tangram.dev/docs/packages), [architecture](https://www.tangram.dev/docs/architecture), [standard SDK](https://github.com/tangramdotdev/packages/blob/main/packages/std/sdk.tg.ts), [bootstrap releases](https://github.com/tangramdotdev/bootstrap/releases).
