# mmr teleport

Status: documented for NHL-340 / NHL-341 (multi-provider teleport)
Date: 2026-05-26

`mmr teleport` moves **exactly one selected coding-agent session** between your
machines so you can continue work on another host. It is **selected-session
handoff**, not ongoing sync (`mmr sync`), store setup (`mmr link`), or
host-wide history export (`mmr export`).

**Current release scope (provider-capable native teleport):**

- Native bundles for **codex**, **claude**, **cursor**, **grok**, and **pi**
  (`.mmr` JSON artifacts with provider-qualified paths such as
  `native/codex/transcript.jsonl`, `native/grok/summary.json`, …)
- Transports: one-shot HTTP (`serve` / `receive mmtp://...`), SSH (`send
  `--to user@host`), and `file://` inbox (`send` / `receive`) — provider-neutral
- `teleport resume --as same|<provider>` and `teleport export --as same|<provider>`
  for same-provider apply/guidance/artifact export; cross-provider transforms return
  `status: "unsupported"` (exit 3)
- **`shared-safe` fidelity is not implemented**; native bundles may contain
  secrets, tool output, private paths, and raw transcript content

### Provider support matrix

| Provider | pack / apply | resume | export primary artifact | notes |
|----------|--------------|--------|-------------------------|-------|
| codex | yes | best_effort (`codex exec resume`) | `native/codex/transcript.jsonl` | baseline |
| claude | yes | manual (`claude --resume`) | `native/claude/transcript.jsonl` | cwd remapped in JSONL |
| cursor | yes | manual (open transcript in IDE) | `native/cursor/transcript.jsonl` | directory layout preserved |
| grok | yes (multi-file) | best_effort | `native/grok/updates.jsonl` | also bundles `summary.json` |
| pi | yes | manual | `native/pi/transcript.jsonl` | session cwd remapped in JSONL |

#### Cursor limitations (NHL-337)

Cursor native teleport is **pack / apply / export** with **manual resume only**:

- One session maps to a single agent transcript JSONL under
  `~/.cursor/projects/<slash-hyphen-project>/agent-transcripts/<session>/<session>.jsonl`
  (or preserves relative layout from source metadata when available).
- `teleport resume --as cursor` applies the bundle then reports manual IDE guidance;
  mmr does **not** invoke a Cursor CLI resume command.
- Cross-provider resume/export (`--as` other than bundle source) returns
  `status: "unsupported"` (exit 3).
- Path remap rewrites transcript path strings; Cursor project directory names use the
  slash-to-hyphen encoding (same as `mmr export` / Cursor loader).

Canonical contract: [specs/teleport.md](../specs/teleport.md).

## Discover the session first

```bash
mmr sessions --project /path/to/project
mmr messages --session sess-abc --project /path/to/project
```

Omit `--project` to use cwd auto-discovery (same as `mmr sessions` / `mmr
messages`). Omit `--session` on teleport commands to select the latest session in scope
(default source filter is codex when `--session` is omitted; pass `--source` for
other providers).

## Workflow 1: Same Tailnet / LAN (one-shot HTTP)

Use when both machines share a Tailscale tailnet or LAN and you want a direct
receive URL without SSH.

**Machine A (sender):**

```bash
mmr teleport serve --session sess-abc --project /path/to/project
```

Stdout prints one JSON object with `listen_url` (for example
`mmtp://100.x.x.x:54321/<token>`), `token`, `expires_at`, and bundle metadata.
The process blocks until one successful download or `--timeout` (default 600s).
Stderr warns that native transfers may contain secrets.

Bind address resolution: `--bind host:port` or `--to host:port`, else
`MMR_TELEPORT_BIND`, else Tailscale IPv4 when available, else `127.0.0.1:0`.

**Machine B (read-only, no native apply):**

```bash
mmr teleport read 'mmtp://100.x.x.x:54321/<token>'
```

`read` downloads the bundle, verifies `X-MMR-Bundle-Sha256`, caches under
`~/.mmr/teleport/cache/<bundle_id>/bundle.mmr`, and returns session metadata plus a
suggested `teleport export` command. Re-reading the same cached path is idempotent
(`status: "skipped"`). It does not write native provider session files.

**Machine B (handoff / resume on this host):**

```bash
mmr teleport receive 'mmtp://100.x.x.x:54321/<token>' --project /path/to/project
```

`receive` downloads the bundle, verifies `X-MMR-Bundle-Sha256`, caches locally,
and applies native provider session files. Use `--force` if a newer local transcript already
exists.

**Resume after apply (optional):**

```bash
mmr teleport resume ~/.mmr/teleport/cache/<bundle_id>/bundle.mmr --project /path/to/project
```

## Workflow 2: SSH machine-to-machine

Use when SSH is the only path between hosts.

**Machine A:**

```bash
mmr teleport send --session sess-abc --project /path/to/project --to bob@macbook
```

When remote `mmr` is on `PATH`, bytes stream into `mmr teleport apply --to -` on
the remote host. When remote `mmr` is missing, the bundle is staged under
`~/.mmr/teleport/inbox/<bundle_id>/` and stdout JSON includes
`status: "partial"` plus `next_command` with the exact remote apply command.

**Machine B (manual apply after partial send):**

```bash
mmr teleport apply --to ~/.mmr/teleport/inbox/tp:v1:.../bundle.mmr --project /path/to/project
mmr teleport resume ~/.mmr/teleport/inbox/tp:v1:.../bundle.mmr --project /path/to/project
```

Preview planned SSH steps without contacting the remote:

```bash
mmr teleport send --session sess-abc --to bob@macbook --dry-run
```

Note: `teleport send` does **not** start HTTP servers; use `teleport serve` for
one-shot HTTP URLs.

## Workflow 3: Shared folder / Syncthing / iCloud / rclone

Use when you already sync a directory between machines. mmr writes an atomic
inbox layout; your sync product moves the folder.

**Writer (Machine A):**

```bash
mmr teleport send \
  --session sess-abc \
  --project /path/to/project \
  --to 'file:///Users/alice/Sync/mmr-inbox'
```

This creates:

```text
/Users/alice/Sync/mmr-inbox/tp:v1:.../
|-- bundle.mmr
|-- bundle.sha256
`-- ready
```

**Reader (Machine B, after sync shows `ready`):**

```bash
mmr teleport receive --to '/Users/bob/Sync/mmr-inbox/tp:v1:...'
```

Or point at the synced inbox entry directory directly:

```bash
mmr teleport receive '/Users/bob/Sync/mmr-inbox/tp:v1:...'
```

Incomplete transfers (`bundle.mmr.partial` only, or no `ready` marker) return
`status: "ok"` with an empty `staged` list - wait for the writer to finish and
sync to complete, then re-run `receive`.

## Workflow 4: Local artifact pack / apply

Use for offline copies, backups, or testing on one machine.

```bash
mmr teleport pack --session sess-abc --project /path/to/project --to ./handoff.mmr
mmr teleport inspect ./handoff.mmr
mmr teleport apply --to ./handoff.mmr --project /path/to/project
```

Default pack output when `--to` is omitted:

`~/.mmr/teleport/bundles/<bundle_id>/bundle.mmr`

Validate before applying on a trusted path:

```bash
mmr teleport inspect ./handoff.mmr
```

## Workflow 5: Resume and export (`--as` convention)

**Resume** applies a bundle and reports provider continuation guidance:

```bash
mmr teleport resume ./handoff.mmr --project /path/to/project
mmr teleport resume ./handoff.mmr --as codex --no-agent-exec
mmr teleport resume ./handoff.mmr --as grok --no-agent-exec
```

- `--as same` (default) resolves to the bundle manifest source.
- Cross-provider targets (for example `--as claude` on a codex bundle) return stdout
  `status: "unsupported"` with exit code **3** (structured JSON, no apply).
- Fidelity tokens `--as native`, `--as shared-safe`, or `--as json` are usage
  errors (exit **2**).

**Export** writes native transcript artifact(s) from a bundle (distinct from
top-level `mmr export`, which queries local history):

```bash
mmr teleport export ./handoff.mmr --to ./exported.jsonl --as codex
mmr teleport export ./handoff.mmr --to ./exported-dir --as grok
mmr teleport export ./handoff.mmr --to ./exported.jsonl --as same
```

Grok export writes a directory (`summary.json` + `updates.jsonl`). Unsupported
cross-provider `--as` values return `status: "unsupported"` with exit
code **3**.

## Safety

| Topic | Behavior |
|-------|----------|
| Scope | One selected session per invocation; not project-wide sync |
| Fidelity | Native provider bundles only; includes secrets, tool I/O, private paths, raw transcript |
| `shared-safe` | **Not implemented** for pack, send, or native resume |
| Transport | User-controlled (SSH keys, tailnet, or folder ACLs); HTTP uses single-use token |
| Warnings | `pack`, `send`, and `serve` print stderr warnings for native sensitivity |
| Idempotency | Re-applying identical content returns `status: "skipped"` unless `--force` |

Do not treat native bundles like sync-grade redacted payloads. Do not upload
native bundles to shared cloud buckets unless you accept full transcript exposure.

## Troubleshooting

### Path remap / project mapping

Bundles carry manifest project paths from the source machine. On apply, pass
`--project /path/on/this/machine` to remap `session_meta.cwd` and related native
paths:

```bash
mmr teleport apply --to ./handoff.mmr --project /Users/bob/dev/mmr
mmr teleport receive mmtp://... --project /Users/bob/dev/mmr
```

Inspect manifest project aliases first:

```bash
mmr teleport inspect ./handoff.mmr
```

### Duplicate session or newer local transcript (`--force`)

If local native session files already exist:

- Identical content hashes -> `status: "skipped"` (exit 0)
- Newer local transcript than bundle `last_timestamp` -> apply fails (exit **3**)
  until you pass `--force`

```bash
mmr teleport apply --to ./handoff.mmr --force --project /path/to/project
```

### Missing remote `mmr` (SSH fallback)

When `teleport send` returns `status: "partial"`, read `next_command` from stdout
JSON and run it on the remote host, or apply manually:

```bash
mmr teleport apply --to ~/.mmr/teleport/inbox/<bundle_id>/bundle.mmr
```

Ensure `mmr` is installed and on `PATH` on the remote host for automatic
streaming apply.

### Corrupt bundle or hash mismatch

Symptoms: `teleport/inspect`, `teleport/apply`, or `teleport/receive` returns
`status: "failed"` (often exit **3**) with hash or parse errors.

Fix: re-pack on the source machine and re-send. Do not apply bundles with
mismatched `bundle.sha256`.

```bash
mmr teleport inspect ./handoff.mmr
mmr teleport pack --session sess-abc --to ./handoff.mmr
```

### Expired or invalid HTTP token

- Wrong token: HTTP 403; bundle is **not** consumed - fix the URL and retry while
  `serve` is still running.
- Timeout: `teleport serve` exits after `--timeout` with no download - re-run
  `serve` on the sender and use the new `listen_url`.
- Second receive after success: fails explicitly once the one-shot server has
  exited; re-run `serve` for another transfer.

### Incomplete file inbox transfer

Empty `staged` with `status: "ok"` usually means:

- `bundle.mmr.partial` still being written, or
- `ready` marker not present yet, or
- sync has not delivered the folder

Wait for the sender to finish and sync to complete, then:

```bash
mmr teleport receive --to '/path/to/inbox/tp:v1:...'
```

## Fresh-session handoff (Linear NHL-321 - NHL-341)

When continuing teleport work in a **new agent session**, use this ticket
sequence to verify status before changing behavior:

| Ticket | ID | Verify |
|--------|-----|--------|
| NHL-321 | TELEPORT-000 | Spec contract in `specs/teleport.md` |
| NHL-322 | TELEPORT-001 | `pack` / `inspect` / `apply` round-trip |
| NHL-323 | TELEPORT-002 | Fast session discovery reused by `pack` / latest-session selection |
| NHL-324 | TELEPORT-003 | Apply path remap, `--force`, Codex resume hints |
| NHL-325 | TELEPORT-004 | Applied bundles readable through `mmr sessions` / `mmr messages` |
| NHL-326 | TELEPORT-005 | `send --to user@host`, partial inbox fallback |
| NHL-327 | TELEPORT-006 | `send --to file://...`, inbox `receive` |
| NHL-328 | TELEPORT-007 | `serve` + `receive mmtp://...` |
| NHL-329 | TELEPORT-008 | `resume`, `export --as`, unsupported exit 3 |
| NHL-330 | TELEPORT-009 | This guide + CLI help alignment |
| NHL-331 | TELEPORT-010 | End-to-end validation, benchmarks, final QA, version bump, artifact build |
| NHL-332 | Multi-provider profile registry | Provider dispatch, provider-qualified artifact paths, unknown-provider errors |
| NHL-333 | Codex profile parity | Codex transcript remap, restore hints, destination paths moved behind profile |
| NHL-334 | Grok profile | Multi-file `summary.json` + `updates.jsonl`, `info.cwd` remap |
| NHL-335 | Claude profile | JSONL under `~/.claude/projects/<encoded>/<session-id>.jsonl` |
| NHL-336 | Pi profile | JSONL under `~/.pi/agent/sessions/<encoded-project>/...` |
| NHL-337 | Cursor profile | JSONL under `~/.cursor/projects/<encoded>/agent-transcripts/<session>/...`, manual resume |
| NHL-338 | Provider-aware resume/export | Same-provider only; cross-provider remains unsupported exit 3 |
| NHL-339 | Provider-neutral transports/read/inspect | `send`, `serve`, `receive`, `read`, and `inspect` preserve source provider |
| NHL-340 | Docs/help | Provider matrix, examples, and CLI help updated |
| NHL-341 | Provider matrix E2E | Table-driven pack/apply/latest/file/read/resume/export tests for all providers |

See [mmr-teleport-validation.md](mmr-teleport-validation.md) for proof surfaces and rerun commands.

Quick verification commands:

```bash
cargo test --test cli_contract teleport
cargo run -- teleport --help
cargo run -- teleport pack --help
```

Confirm implementation-status notes at the top of `specs/teleport.md` match the
behaviors you observe. Prefer fixture-driven tests in `tests/cli_contract.rs`
over manual history when validating changes.

## Environment variables

| Variable | Purpose |
|----------|---------|
| `MMR_TELEPORT_TRANSPORT` | Default send transport (`auto`, `ssh`, `file`) |
| `MMR_TELEPORT_BIND` | Default HTTP bind host for `teleport serve` |
| `MMR_AUTO_DISCOVER_PROJECT` | Cwd project discovery (`0` disables) |
| `MMR_DEFAULT_SOURCE` | Default `--source` filter |

## Output contract

All teleport subcommands emit **one JSON object on stdout** (except fatal clap
errors before dispatch). Human diagnostics, warnings, and agent manual steps go
to **stderr** only. Use `--pretty` for indented JSON.
