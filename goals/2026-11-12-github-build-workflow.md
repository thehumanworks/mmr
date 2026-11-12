---
goal_id: "2026-11-12-github-build-workflow"
title: "Build the mmr CLI for Linux and macOS in GitHub Actions"
description: "Add a GitHub Actions workflow that builds release binaries of mmr for Linux and macOS on x86_64 and arm64, smoke-tests them, and publishes them as artifacts (and as release assets for v* tags)."
date: "2026-11-12"
status: "in-progress"
confidence_floor: 85
created: "2026-11-12"
updated: "2026-11-12"
---

# Goal

Every push to `main`, every pull request, and every `v*` tag builds an
optimized `mmr` binary for four targets on native GitHub-hosted runners:

| Target                      | Runner             |
| --------------------------- | ------------------ |
| `x86_64-unknown-linux-gnu`  | `ubuntu-22.04`     |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04-arm` |
| `x86_64-apple-darwin`       | `macos-15-intel`   |
| `aarch64-apple-darwin`      | `macos-15`         |

Each job runs the binary it built, packs it as `mmr-<target>.tar.gz` with a
`.sha256` file, and uploads both as a workflow artifact. A `v*` tag also
attaches the archives and a combined `SHA256SUMS` to the GitHub release for
that tag.

The user request ("update this project to include a github workflow that builds
the 'mmr' CLI for linux and macos") is the approval to execute.

## Scope and invariants

- Add `.github/workflows/build.yml`. Update `README.md` (prebuilt binaries) and
  `AGENTS.md` (project structure). No Rust, Cargo, or flake changes.
- Builds use `cargo build --locked --release` so CI builds what `Cargo.lock`
  pins.
- Least privilege: the workflow token is `contents: read`; only the release
  job gets `contents: write`, and only on `v*` tags. Checkout does not keep
  credentials. Third-party actions are pinned to full commit SHAs.
- Tag builds do not restore the Rust cache, so release binaries never build
  from a cache that a branch or PR run wrote.
- Portability: Linux binaries build on Ubuntu 22.04 (glibc 2.35 baseline);
  macOS binaries set `MACOSX_DEPLOYMENT_TARGET=11.0`.

## Definition of Done

- [x] DoD-1 — `actionlint` reports no findings for the workflow; verify by
  command output.
- [x] DoD-2 — The build, smoke test, and package commands work locally on
  `aarch64-apple-darwin`; verify by command output and archive listing.
- [x] DoD-3 — The workflow runs green on GitHub for all four targets on a pull
  request; verify by `gh run view`.
- [x] DoD-4 — A downloaded artifact contains `mmr`, `README.md`, and a matching
  checksum; the Linux binary needs glibc ≤ 2.35 and the macOS binary has
  `minos 11.0`; verify by inspecting the downloaded files.
- [x] DoD-5 — README tells users where to get prebuilt binaries; AGENTS.md
  lists the workflow; verify by review.
- [ ] DoD-6 — Release `v0.2.0` exists with all assets, and the README install
  snippet installs a working `mmr` from it; verify by `gh release view` and
  running the snippet in a scratch directory.

## Tasks

### T1 · Workflow · [x]

Matrix build, smoke test (`--version`, `--help`, `skill load`), packaging,
artifact upload, tag-gated release job.

### T2 · Docs · [x]

README "Download a prebuilt binary" section and AGENTS.md structure entry.

### T3 · Verification and PR · [x]

actionlint, local dry run of the build and package steps, GitHub run on a PR,
artifact inspection.

### T4 · Publish the first release · [ ]

User follow-up: "publish the first version when done". After the PR run is
green, merge PR #29, tag the merge commit `v0.2.0` (the `Cargo.toml` version;
the workflow rejects a tag that does not match), push the tag, and confirm the
release job attaches four archives, four `.sha256` files and `SHA256SUMS`.
Verify by installing from the release with the README snippet.

## Exit Conditions

- DONE — all DoD items verified.
- BLOCKED-DEP — a runner label or action version is unavailable.
- SCOPE-CHANGE — a target needs Rust or Cargo changes to build.

## Evidence

- 2026-11-12 — Clean `main` at `5583b81`; no `.github/` directory existed.
  Repository is public, so `ubuntu-22.04-arm` is available. Runner labels
  checked against `actions/runner-images` README; all four images ship rustup.
  Action SHAs resolved with `gh api repos/<action>/commits/<tag>`.
- 2026-11-12 — `actionlint` 1.7.12 with ShellCheck 0.11.0: no findings.
- 2026-11-12 — Local dry run on `aarch64-apple-darwin` with the workflow's
  env: `cargo build --locked --release --target aarch64-apple-darwin` passed;
  smoke test printed `mmr 0.2.0`; package step produced an archive with `mmr`
  and `README.md`; `shasum -a 256 -c` reported OK; `vtool -show-build` shows
  `minos 11.0`. The tag guard accepted `v0.2.0` and rejected `v0.3.0`.
- 2026-11-12 — PR #29 run `43295131570` on `4217e3c`: all four build jobs
  passed (Linux arm64 2m26s, Linux x86_64 3m23s, macOS arm64 5m54s, macOS
  Intel 8m31s; no warm cache). The release job was skipped, as expected for a
  pull request.
- 2026-11-12 — Downloaded all four artifacts with `gh run download`: every
  `.sha256` verified with `shasum -a 256 -c`; every archive holds `mmr` and
  `README.md`. Linux binaries are dynamically linked PIE ELF (x86-64,
  aarch64) whose newest glibc symbol is `GLIBC_2.34`, inside the documented
  2.35 floor. Both macOS binaries report `minos 11.0`. On this Mac the arm64
  binary and the Intel binary (under Rosetta) both printed `mmr 0.2.0`.

## Decisions

- 2026-11-12 — glibc targets on Ubuntu 22.04 instead of static musl builds.
  Native runners build and smoke-test both architectures without cross tools,
  and glibc's allocator avoids musl's slowdown under rayon's parallel ingest.
  The cost is a glibc 2.35 floor (Ubuntu 22.04, Debian 12 and newer). Moving to
  musl later only changes the matrix and adds `musl-tools`.
- 2026-11-12 — The x86_64 macOS build runs natively on `macos-15-intel` so the
  smoke test can execute it. GitHub plans to retire Intel macOS images; when
  that happens, cross-compile from the arm64 runner and drop that smoke test.
- 2026-11-12 — Archive names carry the target only (`mmr-<target>.tar.gz`), so
  `releases/latest/download/mmr-<target>.tar.gz` is a stable download URL.
- 2026-11-12 — The release job uses the preinstalled `gh` CLI rather than a
  third-party release action, to keep the trusted action set small.

## Learnings
