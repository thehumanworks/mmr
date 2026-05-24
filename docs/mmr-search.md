# mmr search

Status: implemented for NHL-273
Date: 2026-05-24

`mmr rg` and `mmr search` provide lexical search over generated local memory
documents. They do not use embeddings or semantic search in the MVP.

## Commands

Exact local search with JSON stdout:

```bash
mmr rg "panic at src/main.rs:42"
```

Structured lexical search:

```bash
mmr search "decision" --role user --session notes
```

Useful filters:

- global `--source codex|claude|cursor|grok|pi`
- `--project <path>`
- `--session <source-session-id>`
- `--role <role>`
- `--event-type <event-type>`
- `--ignore-case`
- `--context <n>`

`mmr rg` treats the pattern as literal text, not as a regular expression. This
keeps shell-special strings such as `ERROR[abc]*` searchable without escaping.
Use `--ignore-case` for case-insensitive matching.

`mmr rg --line` is the explicit POSIX-oriented exception to the JSON stdout
contract. It emits tab-separated fields so the `mmr://` citation remains a
single field:

```text
mmr://event/<event-id>	<line>	<source>	<snippet>
```

## Citations

Every JSON result includes:

- `project_id`
- `source`
- `session_id`
- `event_id`
- `event_type`
- `role`
- `timestamp`
- `citation`
- matched `line_number`, `snippet`, and context lines

The citation is stable as `mmr://event/<event-id>` and can be used by future
show/open commands.

## Search Documents

Search runs rebuild missing `search_documents` rows from normalized events before
matching. The MVP stores one readable document per event, using the event content
and citation metadata. These documents are local search material, not remote sync
payloads.

## Tree Export

For external tools:

```bash
mmr export --format tree --project /path/to/project --output-dir /tmp/mmr-tree
rg "decision" /tmp/mmr-tree
```

Tree export writes one Markdown file per normalized event, grouped by source and
session inside a fresh `mmr-tree-*` run directory below `--output-dir`. It never
mixes a new export with stale files from a previous narrower or broader export.
It requires `--output-dir` so the CLI never writes a tree into the current
directory by surprise. Default `mmr export` behavior remains unchanged and still
returns the raw retrieval JSON contract.

Search and tree export omit local raw refs by default. Stable citations use
`mmr://event/<event-id>`; local source refs remain private diagnostic material
for future explicit inspection commands.
