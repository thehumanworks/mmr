---
goal_id: "2026-11-12-public-repo-install-docs"
title: "Drop private-repository install caveats"
description: "The repository is public, so install docs no longer need GitHub credentials or a 'once public' caveat."
date: "2026-11-12"
status: "done"
confidence_floor: 90
created: "2026-11-12"
updated: "2026-11-12"
---

# Goal

Tomas confirmed the repository is public. Install instructions should not tell
users they need GitHub credentials or that Cargo install works only "after this
repository is public".

## Scope and invariants

- Docs only: `docs/mmr-nix.md` and `README.md`. No code or flake changes.

## Definition of Done

- [x] DoD-1 — No private-repository caveat remains in the install docs; verify
  by `grep -n "private\|access-tokens\|is public" README.md docs/mmr-nix.md`.

## Evidence

- 2026-11-12 — Removed the `access-tokens` / `git+ssh` paragraph from
  `docs/mmr-nix.md` and the "After this repository is public" lead-in from
  `README.md`. The grep shows only the unrelated note that bundles can contain
  private conversation text. `git diff --check` clean.
