---
title: "Demonstrate portable read-only context with a runnable example"
description: "Show file export/copy/import in isolated synthetic homes without native writes."
date: "2026-10-22"
status: "done"
---

## Outcome and scope

Add a small Python-standard-library example to the existing portability branch,
link it from the sharing guide, and show its invocation and expected output.
Use a reusable synthetic fixture helper, two temporary homes/projects, the real
CLI, and byte comparisons of provider files. Add one concise contributor rule
requiring value-demonstrating examples when applicable. No product behavior or
PR-state changes; do not merge.

## Validation and definition of done

- [x] Run the documented example successfully; messages survive copying and
  native histories stay byte-identical in both homes.
- [x] Full format/test/benchmark/Clippy/release gates and independent review pass.
- [x] Commit and push only scoped files to PR 23; retain ready/unmerged state.

## Evidence

- Initial checkout clean at `255e91ddd2157ce709a938af53d303e4f7ae7876`.
  PR 23 is ready and unmerged. Existing snippets require user-provided history;
  no `examples/` directory exists.

## Decisions

- Use one shared fixture helper with identical contents on the two independent
  PR branches, so their eventual combination does not duplicate setup.
- User authorized updates to existing ready PRs and explicitly prohibited merge.

- 2026-10-22 — `cargo build --locked` and
  `python3 -B examples/portable_read_only.py` passed; output matched the guide,
  source/destination snapshots matched, and only temporary files were removed.
- 2026-10-22 — Full verification passed: format check, 360 tests, four ignored
  benchmarks, strict Clippy and release build. Logs: `../evidence/portable-example*`.
  Independent reviewer approved without findings; `git diff --check` clean.
  Manual goal validation used because the prescribed global GDD checker is absent.
