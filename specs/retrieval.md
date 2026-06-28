# Search-to-Read Retrieval

Status: intended contract for implementation
Date: 2026-06-28

## Human

`mmr retrieve <query>` is the one-command context recovery path for agents and
humans who remember a prior phrase, error string, decision, or file path but do
not remember the relevant session id.

Before this feature, the user had to run `mmr find`, inspect matches, copy
session ids, run `mmr read session`, and trim the result. The completed feature
returns a compact JSON packet with ranked sessions, exact event citations, and
bounded message windows around the matches.

This is the right version because it composes existing local capabilities:
literal event search, provider session reads, stable JSON output, and fixture
tests. It keeps the default scoped to the linked current project, but exposes
explicit broad-scope flags for agents that need to recover context across the
whole local history system or across every supported harness.

### User workflow

```bash
mmr retrieve "panic at src/main.rs:42" --pretty
mmr --source codex retrieve "migration decision" --project /path/to/project
mmr retrieve "Modal sandbox fix" --all-projects --all-sources --pretty
mmr retrieve "Modal sandbox fix" --all-projects --all-sources --debug --pretty
mmr retrieve "panic at src/main.rs:42" --full-message-history
mmr retrieve "ERROR[abc]*" --ignore-case -C 2
```

The output is JSON on stdout. The default response is capped at three sessions
and returns ranked sessions plus short `matches[]` snippets. Full provider
message windows are opt-in with `--full-message-history`. Empty matches are
successful JSON responses with a suggested next action.

### Discarded alternatives

- `mmr find --read`: rejected for v1 because `find` should remain exact search,
  including its `--format line` exception.
- Semantic/vector search: rejected for v1 because the source material and tests
  already support literal search, and semantic retrieval would need a separate
  indexing, privacy, and ranking contract.
- Automatic summarization: rejected because retrieved context must remain
  source-backed and auditable through `mmr://event/...` citations.
- Ambiguous `--all` and remote retrieval: rejected because broad local retrieval
  needs explicit axes (`--all-projects`, `--all-sources`) and remote fan-out
  needs its own identity and timeout contract.
- First-class MCP `mmr_retrieve`: rejected; MCP clients can still shell
  out to the CLI or use the existing manual prompt.

## Agent

### Source material

Observed source facts:

- `docs/mmr-search.md` documents `mmr find` as literal local search over generated
  memory documents with JSON and line output.
- `src/cli.rs` defines current `SearchTextArgs` with `query`, `--project`,
  `--session`, `--role`, `--event-type`, `--ignore-case`, `-C/--context`, and
  `--format`.
- `src/cli.rs` currently serializes `SearchResult.session_id` from
  `EventRecord.session_id`, which is the Store-internal id.
- `src/store.rs` exposes both internal `EventRecord.session_id` and public
  `EventRecord.source_session_id`; retrieval must use the latter for readable
  provider sessions.
- `src/types/api.rs` defines `ApiMessage` with `session_id`, `source`,
  `project_name`, role/content/model/timestamp/token fields, and optional origin.
- `specs/messages.md` documents concrete-session pagination stability for read
  flows.
- `tests/memory_fabric_contract.rs` already proves `find` is literal, preserves
  `mmr://event/...` citations, and omits `raw_local_ref`.

### Command contract

Add a top-level CLI command:

```bash
mmr retrieve <query> [flags]
```

Supported flags:

| Flag | Meaning | Default |
| --- | --- | --- |
| `--project <path>` | Explicit project scope. Without it, use linked cwd project behavior like `find`. | cwd-linked project |
| `--all-projects` | Search every local project discovered from loaded provider transcripts instead of the cwd-linked or explicit project. Mutually exclusive with `--project`. | false |
| global `--source <source>` | Source filter for matching and reads. | `MMR_DEFAULT_SOURCE` or all sources |
| `--all-sources` | Search every source/harness and ignore `MMR_DEFAULT_SOURCE`. Mutually exclusive with global `--source`. | false |
| `--debug` | Include retrieve execution metadata under top-level `debug`, including searched projects, project counts, source scope, limits, and ranked-session count. | false |
| `--full-message-history` | Include bounded provider message windows in `selected_sessions[].messages` plus `message_window` and message pagination fields. | false |
| `--session <id>` | Restrict matching to one public provider source session id. | none |
| `--role <role>` | Event role filter, same as `find`. | none |
| `--event-type <type>` | Event type filter, same as `find`. | none |
| `--ignore-case` | Case-insensitive literal matching. | false |
| `-C`, `--context <n>` | Match-context lines in `matches[]`. | 0 |
| `--max-sessions <n>` | Maximum selected readable sessions. | 3 |
| `--before-messages <n>` | Messages before each matched anchor. | 3 |
| `--after-messages <n>` | Messages after each matched anchor. | 12 |
| `--max-messages-per-session <n>` | Hard cap after per-session window merge. | 24 |
| `--limit <n>` | Flattened provider-message page limit when `--full-message-history` is set. | `max_sessions * max_messages_per_session`, 72 by default |
| `--offset <n>` | Flattened provider-message page offset when `--full-message-history` is set. | 0 |
| `--pinned-session <json>` | Concrete continuation identity with `source`, `project_name`, `source_session_id`. Repeatable. | none |

Out of scope: ambiguous `--all`, `--remote`, semantic/vector search, automatic
summarization, legacy `search`/`rg` aliases, and a first-class MCP retrieval
tool.

### Scope semantics

Default scope is deliberately narrow:

```text
project scope = linked cwd project unless --project or --all-projects is set
source scope = MMR_DEFAULT_SOURCE when set, otherwise all sources
```

`--all-projects` searches every local project discoverable from loaded provider
transcripts across the selected source scope. Linked Store projects and learned
memory are included when present, but Store linkage is not required for a
provider session to be searched. It does not import new history or query remotes.

`--all-sources` makes the source scope all supported harnesses even when
`MMR_DEFAULT_SOURCE` is configured. It is the explicit "cross-harness" override
for agents running inside an environment that pins a default source for ordinary
commands.

Invalid combinations fail before matching:

```bash
mmr retrieve "query" --project /tmp/app --all-projects
mmr --source codex retrieve "query" --all-sources
```

Both return structured JSON errors on stdout and diagnostics on stderr.

Broad-scope retrieval is exhaustive over the loaded local corpus. Limits such as
`--max-sessions`, `--max-messages-per-session`, and `--limit` apply after
matching and ranking. They must not restrict which projects, sources, sessions,
or messages are searched. The local source loaders already parallelize harness
discovery; retrieve also parallelizes the literal scan over provider messages
when using `--all-projects`.

Successful default stdout intentionally omits execution metadata such as searched
project lists, project counts, and scope diagnostics. `--debug` adds those under
top-level `debug.scope`; it does not include provider `messages[]` unless
`--full-message-history` is also set.

### Matching and identity

Retrieval reuses literal search semantics over normalized event search documents.
It may share code with `find`, but it must not change `find` output.

Event-backed readable matches group by:

```text
(source, project_name, source_session_id)
```

Do not group or pin by Store-internal `session_id`.

Store-to-read mapping joins:

```text
Store::EventRecord.source_session_id == ApiMessage.session_id
Store/source resolved project identity == ApiMessage.project_name
Store::EventRecord.source == ApiMessage.source
```

Provider-direct broad-scope matches group by the same public identity:

```text
ApiMessage.source
ApiMessage.project_name
ApiMessage.session_id
```

Provider-direct matches that are not backed by a Store event use
`mmr://message/message:v1:...` citations. Store-backed matches keep
`mmr://event/...` citations.

Fixtures must include Codex plus at least one provider with encoded project
names, such as Claude or Cursor.

### Ranking

Rank selected session groups by:

1. `match_count desc`
2. `latest_match_timestamp desc`
3. `source asc`
4. `project_name asc`
5. `source_session_id asc`

Tests must include equal-count and equal-timestamp ties.

### Message windows

For each selected session:

1. Locate each matched anchor in provider messages by direct source identity when
   possible.
2. If no direct map exists, use nearest timestamp in the same session.
3. Merge overlapping windows.
4. Dedupe messages.
5. Preserve chronological order.
6. Cap at `max_messages_per_session`.

When merged windows exceed the cap, keep matched anchor messages first, then the
nearest surrounding context in chronological order until the cap is reached. If
anchors alone exceed the cap, keep anchors chronologically up to the cap and set
`message_window.truncated = true`.

### Unreadable matches

`unreadable_matches` is always present as an array.

Put a match there instead of in `selected_sessions` when:

- it is learned memory (`mmr://learned-memory/...`);
- the Store event has no readable provider messages;
- the source/project/session identity cannot be resolved.

Each unreadable entry includes `citation`, `reason`, `source`, `project_id` or
`project_name` when known, `event_id`, `event_type`, `role`, `timestamp`,
`line_number`, `snippet`, `before`, and `after`.

### Response shape

Minimum JSON fields:

```json
{
  "query": "panic at src/main.rs:42",
  "total_matches": 2,
  "total_selected_sessions": 2,
  "selected_sessions": [
    {
      "rank": 1,
      "source": "codex",
      "project_name": "/tmp/project",
      "source_session_id": "sess-codex-1",
      "rank_reason": {
        "match_count": 2,
        "latest_match_timestamp": "2026-06-28T08:00:00Z",
        "tie_break": ["codex", "/tmp/project", "sess-codex-1"]
      },
      "match_count": 2,
      "first_match_citation": "mmr://event/event-id",
      "matches": [
        {
          "source": "codex",
          "project_name": "/tmp/project",
          "source_session_id": "sess-codex-1",
          "event_id": "event:v1:abc123",
          "event_type": "message",
          "role": "assistant",
          "timestamp": "2026-06-28T08:00:00Z",
          "citation": "mmr://event/event-id",
          "line_number": 1,
          "snippet": "panic at src/main.rs:42",
          "before": [],
          "after": []
        }
      ]
    }
  ],
  "unreadable_matches": [],
  "suggested_next_action": null
}
```

Default selected sessions do not include full provider `messages[]`, provider
message internals, `message_window`, top-level `limits`, top-level `scope`,
top-level `total_ranked_sessions`, or message-pagination fields.

With `--debug`, the response adds:

```json
{
  "debug": {
    "limits": {
      "max_sessions": 3,
      "before_messages": 3,
      "after_messages": 12,
      "max_messages_per_session": 24,
      "limit": 72,
      "offset": 0
    },
    "scope": {
      "all_projects": false,
      "all_sources": true,
      "source_filter": null,
      "total_projects_searched": 1,
      "projects": ["/tmp/project"]
    },
    "total_ranked_sessions": 2
  }
}
```

With `--full-message-history`, each selected session adds `message_window` and
`messages[]`. `messages[]` uses the existing `ApiMessage` shape and is the only
retrieve surface that exposes provider message internals such as model or token
fields.

### Pagination and continuation

Message-window pagination is only active when `--full-message-history` is set.
Default snippet-only output does not paginate hidden provider messages and omits
`next_page`, `next_offset`, and `next_command`.

When `--full-message-history` is set, flatten page order is selected session
rank ascending, then each selected session's messages chronological.

`selected_sessions[]` remains present for every pinned selected session on every
page. `matches[]` remains complete. `messages[]` is page-specific.

When `next_page` is true, `next_command` pins selected session identities:

```bash
mmr --source codex retrieve 'panic at src/main.rs:42' \
  --full-message-history \
  --project '/tmp/project with spaces' \
  --pinned-session '{"source":"codex","project_name":"/tmp/project with spaces","source_session_id":"sess-codex-1"}' \
  --limit 10 --offset 10
```

The command must execute as printed by zsh/bash on macOS. Tests must include a
query or project path with spaces or shell-sensitive characters. Do not prove
this by splitting on whitespace.

Continuation commands preserve explicit broad-scope flags. A first page that
uses `--all-projects --all-sources` must emit a `next_command` that includes
both flags rather than narrowing to the first selected project or inheriting a
future `MMR_DEFAULT_SOURCE` value.

Continuation commands also preserve `--debug` and `--full-message-history` when
those flags were present.

Pinned sessions freeze selected session identities when newer sessions land
later. They do not guarantee a snapshot if provider files mutate inside an
already pinned session between page reads.

Malformed JSON, missing or extra fields, and stale identities return structured
errors. A stale identity uses `pinned_session_not_found` and never falls back to
fuzzy selection.

### Empty matches

No matches is a success:

```json
{
  "query": "missing",
  "total_matches": 0,
  "total_selected_sessions": 0,
  "selected_sessions": [],
  "unreadable_matches": [],
  "suggested_next_action": "Try --ignore-case, a shorter literal query, or mmr find for raw match inspection."
}
```

### Verification

Targeted checks:

```bash
cargo test --test memory_fabric_contract retrieve_ -- --nocapture
cargo test --test cli_contract retrieve_ -- --nocapture
cargo test --test cli_contract retrieve_all_ -- --nocapture
cargo test --test memory_fabric_contract rg_cli_contract_is_implemented -- --exact
cargo test --test memory_fabric_contract search_cli_contract_is_implemented -- --exact
```

Full loop:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

### Required behavior coverage

- parser accepts all supported flags and rejects out-of-scope continuation errors;
- Store events map to provider sessions through public `source_session_id`;
- ranking covers equal-count and equal-timestamp tie-breaks;
- `mmr://event/...` citations survive;
- window merging, truncation, anchor overflow, and nearest-timestamp fallback are
  deterministic;
- learned-memory-only and DB-only matches land in `unreadable_matches[]`;
- `next_command` executes as printed and stays on pinned sessions when a newer
  matching session appears;
- malformed/stale `--pinned-session` errors are structured;
- project, source, `MMR_DEFAULT_SOURCE`, session, role, event type,
  `--ignore-case`, `-C`, and `--context` filters match the docs;
- default output omits `debug`, `scope`, `limits`, `messages[]`,
  `message_window`, and provider-message internals;
- `--debug` includes `debug.scope`/limits without `messages[]`;
- `--full-message-history` includes `messages[]`/`message_window` and activates
  message-window pagination;
- `next_command` preserves `--debug` and `--full-message-history`;
- `raw_local_ref` does not leak;
- existing `find` JSON and line output remain unchanged.
