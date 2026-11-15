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
- Each repository needs build metadata. There is no universal inference of arbitrary project builds.
- The Mac tests used an existing Apple SDK. They do not prove a bare Mac needs no setup.
- On NixOS the Pixi CLI starts, but its dynamically linked build backends and built Linux binaries require an FHS-compatible environment. The experiment uses a temporary Nix `buildFHSEnv`, not a system modification. This does not establish plain-NixOS portability without extra setup.
- tny's Linux recipe needs glibc 2.34 headers and unprefixed binutils in addition to the compiler packages. It builds its pinned native dependencies through the existing Makefile.
- No live LLM calls or personal session histories are used. Runtime checks use synthetic homes and local CLI operations.
- Source pins do not make writable installed caches tamper-proof.

## Evidence

Results and final commit IDs are recorded after all launched checks finish. Raw logs live in `/Users/tomas/repo-runner-evaluation/logs` on the Mac and `$HOME/repo-runner-evaluation/logs` on the Linux host. Application code is unchanged.
