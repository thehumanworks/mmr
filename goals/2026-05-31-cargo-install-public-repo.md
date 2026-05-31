---
title: "Document cargo install from public repo"
description: "Make mmr clearer to install from GitHub by declaring the Rust version and adding README install instructions."
date: 2026-05-31
status: done
---

# GOAL: Cargo Install From Public Repo

## Outcome

Make the repository ready for users to install `mmr` directly with Cargo from
GitHub after the repository is public.

## Surface Touched

- `Cargo.toml` package metadata.
- Root `README.md` install documentation.

## Validation Plan

- Confirm `Cargo.toml` declares the minimum Rust version required by edition
  2024.
- Confirm README shows `cargo install --git` examples and basic prerequisites.
- Run the repository verification loop before closing the goal.

## Definition of Done

The package declares a clear Rust MSRV, the README explains GitHub-based Cargo
installation with pinned examples, and the standard verification loop passes.

## Completion Evidence

- `Cargo.toml` declares `rust-version = "1.85"` alongside edition 2024.
- `README.md` documents `cargo install --git https://github.com/thehumanworks/mmr.git --locked`,
  pinned tag/revision forms, prerequisites, and a smoke check.
- `cargo metadata --format-version 1 --no-deps` reports
  `rust-version=1.85 edition=2024`.
- `cargo install --path . --locked --root <tempdir>` installed `mmr`, and the
  installed binary ran `mmr --help`.
- `cargo fmt`, `cargo test`,
  `cargo test --test cli_benchmark -- --ignored --nocapture`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo build --release` passed.
