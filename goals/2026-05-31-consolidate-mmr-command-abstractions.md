---
title: "Consolidate mmr command abstractions"
description: "Propose a cleaner mmr command model that supports project-wide cross-source context, harness-wide cross-project analysis, and previous-session recall."
date: 2026-05-31
status: done
---

# GOAL: Consolidate `mmr` Command Abstractions

## Outcome

Propose a consolidated command model for `mmr` that supports:

- project-specific memory/context across all sources,
- harness/source-specific learning across all projects,
- immediate previous-session recall.

## Surface Touched

- Current command taxonomy and selectors.
- Proposed command names, selectors, and deprecations.
- Relationship between retrieval, summarisation, assimilation, and raw exports.

## Validation Plan

- Inspect current CLI command surface and existing specs/docs.
- Separate user-facing workflows from internal implementation structure.
- Identify commands/flags to keep, rename, alias, or remove.

## Definition of Done

The proposal names the target abstractions, gives concrete example commands for
the three behavior-driven interactions, and explains which existing surfaces
should be consolidated or deprecated.
