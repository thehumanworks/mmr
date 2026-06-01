---
title: "External source adapter protocol"
description: "Define a small JSON-over-stdio protocol so mmr can ingest future coding-agent histories without hard-coding every provider."
date: 2026-06-01
status: proposed
---

# GOAL: External Source Adapter Protocol

## Outcome

Define and implement a minimal external source adapter protocol for `mmr`.
External binaries named `mmr-source-<name>` should be discoverable and able to
declare read/import capabilities over JSON on stdin/stdout.

The first protocol version should cover read-only and import-oriented history
ingestion:

- `info`: source name, display name, protocol version, supported capabilities,
  default roots, parser version.
- `detect`: whether the source is installed or has readable local history.
- `list-sessions`: session ids, project aliases, timestamps, message counts.
- `read-session`: normalized messages or events for one session.
- `project-aliases`: optional source-native aliases for a canonical project path.

## Why

Entire's external agent protocol is one of its strongest architecture choices:
the main CLI can integrate new agents without merging all provider-specific
logic into core. `mmr` currently hard-codes every source into enums, loaders,
imports, and teleport profiles. A small adapter protocol would keep the core
stable while allowing experimental providers to prove value before becoming
first-party.

## Surface Touched

- New protocol spec under `specs/` or `docs/references/`.
- Discovery and source registry in `src/source/`, `src/capture.rs`, and
  `src/types/domain.rs`.
- CLI help and source validation.
- Contract tests for adapter discovery, invalid protocol versions, stdout/stderr
  discipline, and malformed adapter output.

## Source Evidence

- Entire discovers external agents from `$PATH` using `entire-agent-<name>`.
- Entire's protocol uses stateless subcommands, JSON over stdin/stdout, explicit
  capabilities, and stderr for errors.
- Entire gates optional behavior behind declared capabilities to avoid calling
  unsupported subcommands.

## Non-Goals

- No hook installation protocol in v1.
- No provider-native teleport protocol in v1.
- No long-running daemon or persistent connection.
- No implicit trust of adapter output; every record is validated before entering
  the `mmr` store.

## Validation Plan

- Add a tiny fixture adapter binary or shell script under tests that implements
  the protocol.
- Test discovery from a temp `PATH`, source filtering by adapter name, project
  lookup, session read, import, and status diagnostics.
- Test protocol failures: missing executable bit, invalid JSON, wrong protocol
  version, duplicate source name, unsupported capability, stderr diagnostics.
- Run full repo verification.

## Definition of Done

- [ ] `mmr` can discover `mmr-source-*` adapters without changing source code.
- [ ] Adapter sessions can be read and imported with the same response contract
      as first-party sources.
- [ ] Adapter failures are structured, non-destructive, and visible in
      `mmr status`.
- [ ] First-party sources keep existing behavior.
- [ ] Full verification loop passes.
