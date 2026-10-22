# mmr session sharing

The simplest handoff is a file: export one selected session on the source,
copy it, then read it on the destination. Native restoration is optional.

## Portable Files Without SSH

```bash
mmr --source codex share session <session-id> --to file:///absolute/path/to/inbox
# Copy the directory reported as inbox_path to the destination machine.
mmr import bundle /copied/entry
mmr import bundle /copied/entry -O md | jq -r .text
```

Use `--source claude` or `--source pi` for those providers. `share session` uses
existing collectors; no provider conversion is needed to read the result in a
different agent. Each entry contains JSON `bundle.mmr`, `bundle.sha256`, and a
`ready` marker published after the partial write. Copy the whole entry to keep
the sidecar check. A standalone `.mmr` file also works, with internal artifact
hash validation. This is not a ZIP archive.

Default imports verify and print messages and may cache the bundle under
`~/.mmr/teleport/cache`. They do not write `.codex`, `.claude` or `.pi` histories,
or ingest events into local search. Messages keep their source project names.
`--project` remaps only native restoration with `--apply`; reading from a
new project directory does not rewrite provenance.

No SSH access, server, cloud account or shared store is required. The current
`sync`/FakeGithubRemote implementation is file-backed simulation, not a hosted
GitHub/S3/R2 service. A central store can be considered separately if needed.

## Optional SSH Reads

`mmr` accepts trusted SSH targets per command. There is no host registry or
background discovery: pass an already reachable target (`mini`, `user@host`,
`user@host:22`, or `ssh://user@host:22`).

Use `--remote <ssh-target>` on read/query commands when you want another machine
to contribute history with the same shape as a local response.

```bash
mmr list projects --remote mini
mmr list sessions --remote mini --project /Users/mish/projects/mmr
mmr read project --remote mini
mmr read session <session-id> --remote mini
mmr read source --source codex --remote mini
mmr context project --remote mini
mmr context source --source codex --remote mini
mmr recall --remote mini
mmr summarize project --remote mini
```

When `--remote` is absent, local-only output remains unchanged. When it is
present, remote projects, sessions, and messages include `origin` metadata:

```json
{
  "transport": "ssh",
  "host": "mini",
  "remote_mmr_version": "0.2.0"
}
```

Pagination commands echo `--remote`, so follow-up reads stay on the same peer
set. A remote failure is strict: if any named peer cannot run the hidden peer
protocol, the command exits nonzero with structured JSON.

## Share From The Source Machine

Use `share` when you are on the machine that has the session and want to make
that session available somewhere else.

```bash
mmr share session latest --project /Users/mish/projects/mmr --to user@host
mmr share session sess-abc --to file:///Users/mish/Sync/mmr-inbox
mmr share session latest --via http --bind 100.x.x.x:0
```

Selectors:

- `latest` or omitted selector chooses the latest session in the project/source
  scope.
- An explicit session id selects that one session.
- `--project` narrows the session search.
- global `--source` narrows the provider.

Destinations:

- SSH target: stream to remote `mmr import bundle --to - --apply` when remote
  `mmr` is on `PATH`; otherwise stage a bundle in the remote inbox and return
  the exact import command to run there.
- `file://` inbox: write an atomic bundle into that directory.
- `--via http`: print a one-shot `mmtp://...` locator, serve until one successful
  download, then exit.

`--dry-run` is available for SSH/file plans. It is intentionally not available
with `--via http` because serving a one-shot locator is the operation itself.

## Import On The Destination Machine

Use `import` when you are on the machine that should read or apply a session
bundle.

```bash
mmr import session --from mini --session latest --project /Users/mish/projects/mmr --read-only
mmr import session --from user@host:22 --session sess-abc --project /Users/mish/projects/mmr --apply
mmr import bundle ./handoff.mmr --read-only
mmr import bundle mmtp://100.x.x.x:PORT/TOKEN --apply --project /Users/mish/projects/mmr
mmr import bundle --to - --apply
```

Modes:

- No mode flag, or `--read-only`, verifies and prints the bundle's messages
  without applying native provider files.
- `--apply` explicitly restores native provider files.
- `--force` allows replacement during apply and requires `--apply`.
- `--read-only` and `--apply` together are a usage error.

**Migration:** older builds applied imports when `--read-only` was absent.
Add `--apply` to scripts that intentionally restore native files. Existing
`--read-only` commands and explicit `--apply` streams keep their behavior.
A bare `--force` is now a usage error. Stdin still requires `--apply`.

`import session --from <remote>` asks the remote peer to pack one selected
session and then reads or applies the bundle locally. The remote does not need to
run a long-lived process; it only needs an up-to-date `mmr` executable reachable
through SSH for the duration of the command.

`import bundle <locator>` reads or applies an existing bundle path, inbox entry,
stdin stream (`--to - --apply`), or one-shot HTTP locator.

## Event Ingestion

Provider history ingestion uses `ingest events`; `import` is reserved for
session/bundle material.

```bash
mmr --source codex ingest events --project /path/to/project
mmr --source claude ingest events --project /path/to/project --source-root /tmp/.claude
mmr --source cursor ingest events --project /path/to/project --source-root /tmp/.cursor
```

## Operational Notes

- stdout remains machine-readable JSON; diagnostics and warnings go to stderr.
- Native bundles can contain private transcript content and local paths. Treat
  them like provider history files.
- The peer target string is not validated against `known_hosts` or any local
  registry before execution. SSH handles trust and reachability.
- The implementation still uses native provider bundle profiles internally so
  Codex, Claude, Cursor, Grok, and Pi can preserve their own storage layouts.
