# mmr retrieve Search-to-Read Pipeline

view: Human

`mmr retrieve <query>` gives users and coding agents a single command for the
workflow they were previously doing by hand:

1. Search for a phrase, file path, or error string.
2. Decide which sessions matter.
3. Read the selected sessions.
4. Trim the output so it is useful as context.

The command returns ranked session packets with exact match citations and short
match snippets by default. It deliberately stays lexical: no embeddings, no
model summary, and no automatic remote fan-out. It starts with the linked
current project, then lets a user or agent opt into broader local scope with
`--all-projects` and `--all-sources`.

Use the broad flags when the clue is likely in another repository or another AI
harness:

```bash
mmr retrieve "sandbox image regression" --all-projects --all-sources --pretty
```

`--all-projects` means every local project discovered from loaded provider
transcripts. `--all-sources` means all supported harnesses, even if
`MMR_DEFAULT_SOURCE` normally narrows commands.

Use `--debug` when you need searched-project and source-scope metadata. Use
`--full-message-history` when you need bounded provider message windows in
`selected_sessions[].messages`; default output stays snippet-only and does not
paginate hidden messages.

For the full product contract, read `specs/retrieval.md`.
