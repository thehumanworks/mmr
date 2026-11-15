---
title: Evaluate source repository runners on Linux and macOS
description: Verify GitHub source builds and execution without GitHub Actions or published tool binaries.
date: 2026-11-15
status: done
---
Outcome: establish whether existing runners support `<runner> <github_repo> -- command` for mmr (Rust) and tny (C/C++).
Surface: experimental build configuration, runner adapters if explicitly labelled, and evidence. No product source changes or main-branch changes.
Authorization: user explicitly requested feature branches and practical testing in this conversation.
Validation: pinned source revisions; real Linux and macOS execution; version/help plus synthetic functional commands; record toolchain provisioning, cold/warm runs, cache behavior and host prerequisites. Compare Pixi and Bazel viability and existing Nix baseline. Do not equate a custom wrapper with native URL support.
Definition of done: reproducible experiment files and evidence, explicit pass/fail/untested verdict for both repositories and platforms, limitations and recommendation. No live provider inference. Never use private session history as fixtures.

Result: both applications passed the generic Pixi GitHub runner on macOS ARM64 and Linux x86_64 in an FHS environment on NixOS; tny passed eight fixture runtime tests on each. Ten wrapper tests pass. Both existing Nix GitHub invocations passed on native NixOS. Native mise Cargo mmr source invocation passed on macOS. Bazel/Buck2 remain feasible but unintegrated; Tangram macOS installer returned404. Evidence and reproduction are in `experiments/repo-runners/README.md` and `evidence/results.json`. Application code and original main worktrees remain unchanged.

Follow-up authorized by user: implement and execute Buck2 builds for both projects on macOS/Linux; investigate Tangram macOS through source bootstrap and real recipe execution. Do not present Make/Cargo adapter actions as fully hermetic native language-rule integrations. Preserve earlier measured results.

Follow-up result: Buck2 GitHub adapter and source-build actions passed mmr/tny on macOS ARM64 and native NixOS x86_64, with source-pinned cold/warm execution and fixture runtime checks. Host toolchains remain an explicit limitation. Tangram compiled from source and passed sandboxed C compile/run with downloaded LLVM/macOS SDK and std.sdk on macOS ARM64; built CLI minOS26. App-specific Tangram and Linux/Intel proofs remain outside this completed Mac investigation. Reports, test revisions and sanitized fixtures are committed under experiments/repo-runners.
