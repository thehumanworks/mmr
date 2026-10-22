---
title: "Demonstrate standalone stdio with a runnable synthetic example"
description: "Initialize MCP and read a fixture session without usable REST settings."
date: "2026-10-22"
status: "done"
---

## Outcome and scope

Add a small Python-standard-library example to PR 22, using the same fixture
helper as the independent portability PR. Run the real stdio protocol with
unusable REST settings, list tools, read a synthetic session, verify native
histories are unchanged, and exit on EOF. Link invocation/expected output from
the generated API guide. No product behavior or PR-state changes; do not merge.

## Validation and definition of done

- [x] Documented example runs in a temporary HOME with a minimal environment;
  initialization, list/read, JSON-only protocol and unchanged native files pass.
- [x] Full format/test/benchmark/Clippy/release gates and independent review pass.
- [x] Commit/push only scoped files to PR 22, retaining ready/unmerged state.

## Evidence

- Clean branch at `2fae4fb0247e457eb3a0fcba4e60d702baf5447c`; ready/unmerged PR.
  Existing MCP contracts cover behavior, but no user-run demo exists.

## Decisions

- Reuse an identical `_synthetic.py` helper on both independent PRs. Add the
  general contributor example rule once in PR 23 to avoid duplicated guidance.
- Use deliberately invalid REST settings to demonstrate standalone operation
  without opening even a fixture network socket.

- 2026-10-22 — `cargo build --locked` and
  `python3 examples/standalone_stdio.py` passed with the documented output.
  Independent reviewer also ran the demo successfully, checked subprocess
  cleanup/isolation, and approved. Its Markdown-spacing nit was corrected in
  the generator and regenerated guide.
- 2026-10-22 — Full verification passed: formatting, 353 tests, four ignored
  benchmarks, strict Clippy, release build and diff check. Logs:
  `../evidence/stdio-example*`. No changes to product behavior. The common
  fixture helper is byte-identical to PR 23's helper.
  Manual goal validation used because the global GDD checker is unavailable.
