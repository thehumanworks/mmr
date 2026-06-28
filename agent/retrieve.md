# mmr retrieve Search-to-Read Pipeline

view: Agent
route: `/agent/retrieve/`
canonical_contract: `/specs/retrieval.md`

## Required Behavior

- `mmr retrieve <query>` remains CLI-only and returns JSON on stdout.
- Default project scope is the linked current project.
- `--all-projects` searches every local project discovered from loaded provider
  transcripts, plus linked Store learned memory when present.
- `--all-projects` conflicts with `--project`.
- Default source scope is `MMR_DEFAULT_SOURCE` when set, otherwise all sources.
- `--all-sources` overrides `MMR_DEFAULT_SOURCE` and searches every supported
  harness.
- `--all-sources` conflicts with global `--source`.
- Default output is concise JSON with ranked sessions, session identity,
  `rank_reason`, `matches[]`, `unreadable_matches[]`, and no searched-project
  scope metadata or provider `messages[]`.
- `--debug` adds top-level `debug.scope`, `debug.limits`, and
  `debug.total_ranked_sessions`; it does not include provider messages by itself.
- `--full-message-history` adds `selected_sessions[].message_window`,
  `selected_sessions[].messages`, and message-window pagination fields.
- `next_command` preserves `--all-projects`, `--all-sources`, `--debug`, and
  `--full-message-history` when they were used.
- Search is exhaustive before output caps are applied. `max_sessions`,
  `max_messages_per_session`, and `limit` must not restrict project/source/session
  traversal.
- The all-projects provider-message scan is parallelized with rayon.
- Selected sessions and pinned continuation use public `source_session_id`, not
  Store-internal `session_id`.
- Broad retrieval stays local-only; `--remote` remains out of scope.

## Proof Commands

```bash
python3 -m json.tool .well-known/agents.json
rg -n 'all-projects|all-sources|debug|full-message-history|scope' specs/retrieval.md agent/retrieve.md
cargo test --test cli_contract retrieve_ -- --nocapture
cargo test --test memory_fabric_contract retrieve_ -- --nocapture
cargo clippy --all-targets --all-features -- -D warnings
```
