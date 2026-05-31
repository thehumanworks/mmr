---
title: "Project alias resolution for mmr queries"
description: "Allow explicit --project values such as a project basename or known provider alias to resolve to the matching local project without requiring an absolute path."
date: 2026-05-31
status: done
---

# GOAL: Project alias resolution for `mmr` queries

## Outcome

`mmr` should treat explicit `--project <value>` as an alias-capable project
selector. A basename such as `mmr` should match sessions/messages whose stored
project name or path basename is `mmr`, while absolute paths and provider-native
encoded aliases keep working.

## Surface touched

- Query project matching in `src/messages/service.rs`.
- CLI contract tests in `tests/cli_contract.rs`.
- Documentation/spec notes for the `--project` selector.

## Validation plan

- Add fixture-driven CLI contract coverage for basename aliases and ambiguous
  alias handling if needed.
- Run focused tests first, then the repo verification loop as practical:
  `cargo fmt`, targeted `cargo test`, and broader checks.

## Definition of done

- `mmr messages --project <basename>` and `mmr sessions --project <basename>`
  resolve matching project history across supported sources.
- Existing absolute-path and cwd behavior remains compatible.
- Tests document the new alias behavior.
