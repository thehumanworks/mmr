---
title: "Auto-start API docs server with Pitchfork"
description: "Configure Pitchfork to start the generated MMR API docs dev server automatically."
date: 2026-07-02
status: done
---

# GOAL: Auto-start API docs server with Pitchfork

## Outcome

Pitchfork can start the generated MMR API docs server automatically, serving
`docs/api` over `0.0.0.0:8765` for local and Tailscale access.

## Surfaces touched

- `pitchfork.toml` in the repository root.
- `scripts/serve-api-docs.py` as the long-running static docs server entrypoint
  for Pitchfork.
- This goal document.

## Validation plan

- Use subagents to extract the relevant Pitchfork TOML/service configuration
  from the local docs without loading the full documentation into root context.
- Validate the resulting `pitchfork.toml` against the documented shape.
- Start or inspect the configured service when practical and verify HTTP access
  to the API docs.

## Definition of done

- [x] `pitchfork.toml` exists and defines the API docs server.
- [x] The configured command serves `docs/api` on `0.0.0.0:8765`.
- [x] The config shape follows the local Pitchfork documentation.
- [x] HTTP verification proves the docs are reachable.
