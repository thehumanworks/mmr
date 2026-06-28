# mmr retrieve Search-to-Read Pipeline

view: Agent

Canonical contract: `specs/retrieval.md`.

Source material:

- `goals/2026-06-18-search-to-read-retrieval-pipeline.md`
- `docs/mmr-search.md`
- `specs/messages.md`
- `src/cli.rs`
- `src/store.rs`
- `src/messages/service.rs`
- `src/types/api.rs`
- `tests/memory_fabric_contract.rs`
- `tests/cli_contract.rs`

Implementation notes:

- Add `mmr retrieve <query>` as a CLI-only v1.
- Preserve existing `mmr find` output and line-mode behavior.
- Match with literal search over normalized events and active learned memory.
- Default to the linked cwd project unless `--project` or `--all-projects` is
  supplied.
- `--all-projects` searches every local project discovered from loaded provider
  transcripts; it does not import new history or query remotes.
- `--all-sources` searches every supported harness and ignores
  `MMR_DEFAULT_SOURCE`; it conflicts with global `--source`.
- Use public `source_session_id`, never Store-internal `session_id`, for selected
  sessions and pinned continuation.
- Report learned-memory-only and DB-only matches in `unreadable_matches[]`.
- Keep default output bounded: 3 sessions, 3 before, 12 after, 24 messages per
  session, derived limit 72.
- Make `next_command` executable as printed by zsh/bash and pin session identities
  with JSON `--pinned-session` values. Preserve `--all-projects` and
  `--all-sources` in continuation commands when they were used.
- Treat docs as source of truth; patch docs before code when ambiguity appears.

Required target checks are listed in `specs/retrieval.md`.
