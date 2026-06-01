# mmr session sharing

`mmr` treats trusted SSH targets as ordinary command locations. There is no
host registry, discovery daemon, or stored peer configuration: pass a target your
machine can already reach (`mini`, `user@host`, `user@host:22`, or
`ssh://user@host:22`) and the command succeeds or fails on that connection.

## Remote Reads

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

- `--read-only` verifies and prints the bundle's messages without applying
  native provider files.
- `--apply` is the explicit apply spelling. Applying is also the default when
  `--read-only` is absent.
- `--force` allows replacing an existing native provider file when applying.
- `--read-only` and `--apply` together are a usage error.

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
