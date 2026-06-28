# mmr Retrieval Docs

view: Agent
route: `/agent/overview/`

Read order:

1. `/.well-known/agents.json`
2. `/agent/retrieve/` or `/agent/retrieve.md`
3. `/specs/retrieval.md`
4. `/goals/2026-06-28-retrieve-all-scope-flags.md`
5. `AGENTS.md` and `.cursor/rules/`

Rules:

- Docs are the source of truth.
- Patch docs before code when ambiguity appears.
- Keep `mmr retrieve` CLI-only for this change.
- Preserve `mmr find` behavior and machine-readable stdout.
