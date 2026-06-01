---
title: "No backwards compatibility command proposal"
description: "Update the mmr remote/share/import proposal assuming no backwards compatibility unless explicitly requested."
date: 2026-06-01
status: done
---

# GOAL: No Backwards Compatibility Command Proposal

## Outcome

Revise the proposed `mmr` command surface for remote reads and session
movement under the user's explicit preference: do not preserve backwards
compatibility unless requested.

## Scope

- Design proposal only.
- No CLI implementation in this pass.

## Definition Of Done

- [x] Record the no-backwards-compatibility preference in memory.
- [x] Remove compatibility aliases from the command proposal.
- [x] Propose the clean breaking command surface.
