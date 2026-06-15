---
title: "Summarize project/session message paging"
description: "Add --limit and --offset to summarize project and session; window from newest message (offset 0 = latest)."
date: 2026-06-03
status: done
---

# GOAL: Summarize `--limit` / `--offset`

## Outcome

`mmr summarize project` and `mmr summarize session` accept `--limit` and `--offset` so the model sees only a slice of messages, with index 0 = most recent (same paging contract as `mmr read project` / `mmr read session`).

## Validation

- `cargo fmt`, `cargo test`, clippy, `cargo build --release`
- Integration tests assert help text and that mocked summarize input contains only the paged messages.

## Definition of done

- CLI flags on both subcommands; unpaged project still uses `remember` when `limit` unset and `offset` is 0.
- Remote merge paths apply the same pagination after dedup/sort.