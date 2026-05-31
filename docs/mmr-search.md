# mmr find

Status: implemented for the breaking command taxonomy
Date: 2026-05-31

`mmr find` provides lexical search over generated local memory documents. It
does not use embeddings or semantic search.

## Command

Exact local search with JSON stdout:

```bash
mmr find "panic at src/main.rs:42"
```

Structured lexical search:

```bash
mmr find "decision" --role user --session notes
```

Useful filters:

- global `--source codex|claude|cursor|grok|pi`
- `--project <path>`
- `--session <source-session-id>`
- `--role <role>`
- `--event-type <event-type>`
- `--ignore-case`
- `--context <n>`

`mmr find` treats the pattern as literal text, not as a regular expression. This
keeps shell-special strings such as `ERROR[abc]*` searchable without escaping.
Use `--ignore-case` for case-insensitive matching.

`mmr find --format line` is the explicit POSIX-oriented exception to the JSON
stdout contract. It emits tab-separated fields so the `mmr://` citation remains
a single field:

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

The citation is stable as `mmr://event/<event-id>`.

## Search Documents

Search runs rebuild missing `search_documents` rows from normalized events before
matching. The store keeps one readable document per event, using event content
and citation metadata. These documents are local search material, not remote sync
payloads.

## Tree Reads

For external tools:

```bash
mmr read project --format tree --project /path/to/project --output-dir /tmp/mmr-tree
rg "decision" /tmp/mmr-tree
```

Tree reads write one Markdown file per normalized event, grouped by source and
session inside a fresh `mmr-tree-*` run directory below `--output-dir`, and print
a JSON manifest on stdout. Search and tree reads omit local raw refs by default.
