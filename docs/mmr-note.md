# mmr note

Status: implemented for NHL-271
Date: 2026-05-24

`mmr note` records human-authored observations as first-class Memory Fabric
events. Notes are not a separate note system; they use the same local store,
search document, redaction, sync, summary, and assimilation path as imported agent
events.

## Usage

Inline text:

```bash
mmr note "decision: use fixture-driven integration tests for cwd behavior"
```

Multiline stdin:

```bash
cat decision.md | mmr note
```

## Project Scope

`mmr note` writes to the currently linked project. If the cwd is not linked, the
command fails with a diagnostic telling the user to run `mmr init`.

During pre-`init` implementation work, tests can create the project link through
the hidden `mmr __db-info --project <path>` smoke command. Public setup remains
owned by NHL-277.

## Storage

Notes are inserted as normalized events with:

- `source = "note"`
- `source_session_id = "notes"`
- `event_type = "note"`
- `role = "user"`
- `parser_version = "note-v1"`
- `sync_status = "local_only"`
- `source_event_id = "note:<timestamp-and-content-hash>"`
- `raw_local_ref = "note://notes/<timestamp-and-content-hash>"`

Each note is inserted atomically with a `search_documents` row and a
`mmr://event/<event-id>` citation so `mmr find` can index it when NHL-273
lands. Note provenance avoids local project identifiers so future sync can
hydrate or replay notes without depending on a host-specific path hash.
