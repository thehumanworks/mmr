# Source repository runner evaluation — 2026-11-15

## Contract

Run a tool as `repo-run github.com/OWNER/REPO@COMMIT -- COMMAND ARG...` on native Linux and macOS. Build source locally and reuse the result. No GitHub Actions runner or upstream application release binary is part of this workflow. Downloading upstream compiler/runtime packages is permitted.

## Scope and inputs

- mmr: Rust, base `daf2639dcd5768c6ad809b1e771b0b9029d0eb24`.
- tny: C11 and private C++20 plus embedded CPython and native libraries, base `eebc162d1b22125afd75409c8a0a530ed66a0be1`.
- Feature branch in each repository: `experiment/repo-runners-20261115`.
- macOS ARM64 with an existing Apple SDK; Linux x86_64 NixOS 26.11.
- Pixi 0.84.2; Bazel 9.3.2.

## Reproduction

The root `pixi.toml` supplies the repository-specific package definition. The generic runner is maintained in the mmr experiment directory and has no knowledge of either application. Install Python 3.9+, Git and Pixi, put `repo-run` on PATH, then use the command contract above. `PIXI_BIN` and `REPO_RUN_CACHE` allow isolated installation and cache locations.

For manual use after checkout: `pixi run --manifest-path pixi.toml -x -- mmr --version` (or `tny --version`). This is a build-on-consumption workflow, not downloading a tool release.

## Important boundaries

- Pixi is the build/dependency engine; `repo-run` is a custom generic GitHub adapter, not a native Pixi feature.
- The tested source packaging uses Pixi's `pixi-build` preview feature.
- Each repository needs build metadata. There is no universal inference of arbitrary project builds.
- The Mac tests used an existing Apple SDK. They do not prove a bare Mac needs no setup.
- On NixOS the Pixi CLI starts, but its dynamically linked build backends and built Linux binaries require an FHS-compatible environment. The experiment uses a temporary Nix `buildFHSEnv`, not a system modification. This does not establish plain-NixOS portability without extra setup.
- tny's Linux recipe needs glibc 2.34 headers and unprefixed binutils in addition to the compiler packages. It builds its pinned native dependencies through the existing Makefile.
- No live LLM calls or personal session histories are used. Runtime checks use synthetic homes and local CLI operations.
- Source pins do not make writable installed caches tamper-proof.

## Other tools

See [mise, Buck2 and packslip](mise-buck2-packslip.md), [the candidate comparison](alternatives.md), [Bazel integration assessment](bazel-assessment.md), and [Tangram evidence](tangram.md). Missing Bazel metadata establishes that the direct invocation is unsupported in these source trees; it does not establish inability to build either program. Bazel and Buck2 need project build rules and a fetch/run adapter for the requested command shape.

## NixOS reproduction

```sh
nix build --impure --file experiments/repo-runners/nixos-fhs.nix --out-link /tmp/runner-evaluation-fhs
/tmp/runner-evaluation-fhs/bin/runner-evaluation-fhs
# Inside this shell, run repo-run normally with Pixi on PATH.
```

The test host rewrites GitHub HTTPS URLs to SSH in its global Git configuration. Tests used process-local `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1` to retain HTTPS. This is a host-specific test setting, not a general installation requirement.

## Evidence

 Raw logs live in `/Users/tomas/repo-runner-evaluation/logs` on the Mac and `$HOME/repo-runner-evaluation/logs` on the Linux host. Application code is unchanged.

### Completed application tests

| GitHub runner | macOS ARM64 | Linux x86_64 (FHS on NixOS) |
| --- | --- | --- |
| Pixi wrapper: mmr | PASS; 39.90s first build/run, 0.96s warm | PASS; 161s first build/run, 1s warm |
| Pixi wrapper: tny | PASS; 260.03s first build/run, 1.21s warm | PASS; 451s first build/run, 1s warm |

These first application builds reused compiler/download caches. Concurrent Linux jobs make timings unsuitable for comparing build engines. Both platforms executed the application built from Git source. Mac mmr checks listed and read synthetic session data; Linux listed an empty synthetic home. Both tny binaries passed help, tool listing and eight existing fixture-only integration tests, including embedded Python and extensions. No real provider credentials or histories were used.

Tested application revisions:
- mmr: `bc5075a0da36824e99e23ef3ce070d11fbf56b3b`
- tny: `e616aea15847afe386025fe60f8d5583f54dae88`

Final wrapper review fixed annotated-tag fetching; ten wrapper tests pass, including a real-Git annotated-tag regression. Warm tny runs used this final wrapper. The repository quality gate for tny passed (`make quality` with isolated uvx clang-format23.1.0, clang-tidy22.1.8 and Ruff0.16.6); Darwin skips its GCC analyzer as expected. No product source files changed.

### Verdict

**Pixi plus the reusable adapter meets the tested two-repository command contract on these hosts.** It downloads the toolchain/dependencies, builds on the consumer machine and caches the result. It requires a root package recipe and uses preview source-packaging support. It is not a zero-prerequisite or zero-compilation system.

For mmr alone, native mise Cargo Git installation passed on macOS with existing Rust1.99.1/Apple SDK, without this wrapper. Native mise `github:` and packslip install published artifacts; they do not replace the tny recipe. Bazel and Buck2 can be integrated for both operating systems, but need explicit build/toolchain definitions and a GitHub adapter. Tangram's source/toolchain model is promising, but its macOS installer failed404 and its CLI does not directly interpret GitHub URLs. Xmake/xrepo also supports both OS families but requires packaging integration; Linux-only Brioche is excluded.

Nix remains the closest native repository interface, and both repositories already have flakes. Both direct GitHub flake invocations passed on native NixOS, including the existing package build/check phases: `nix run github:thehumanworks/mmr/daf2639dcd5768c6ad809b1e771b0b9029d0eb24 -- --version` printed `mmr 0.2.0`; `nix run github:thehumanworks/tny/eebc162d1b22125afd75409c8a0a530ed66a0be1 -- --version` printed `eebc162`. Mac Nix execution was not tested. The Nix mmr build includes release-mode tests, unlike the Pixi recipe, so its build duration is not a like-for-like performance comparison.

### Run the experiment

Install Python3.9+, Git and Pixi, then place mmr's `experiments/repo-runners/repo-run` on PATH. This single file works for either repository and any other repository with an appropriate root Pixi manifest. Use full revisions for repeatable source selection:

```sh
repo-run github.com/thehumanworks/mmr@bc5075a0da36824e99e23ef3ce070d11fbf56b3b -- mmr --version
repo-run github.com/thehumanworks/tny@e616aea15847afe386025fe60f8d5583f54dae88 -- tny --version
```

The feature-branch name can replace the SHA to follow changes. The runner refreshes named refs; pinned source can be reused without another Git fetch. Pixi may still need network access for uncached dependencies. The caller's working directory and command arguments are preserved. Python and Git remain bootstrap prerequisites; macOS SDK and Linux runtime compatibility are separate requirements.

Machine-readable outcomes: [results.json](evidence/results.json). The committed runtime smoke script in the tny experiment directory takes the installed binary and checkout path as its two arguments.
