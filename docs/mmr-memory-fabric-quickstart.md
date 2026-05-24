# mmr Memory Fabric Quickstart

Status: implemented through NHL-281. See
`docs/mmr-memory-fabric-release-gate.md` for final release evidence.

This guide covers the lean MVP flow from a blank non-Git directory to linked,
synced, searchable, and dreamed local memory.

## One-Time Setup

Create or enter any project directory. It does not need to be a Git repository.

```bash
mkdir -p /tmp/mmr-demo
cd /tmp/mmr-demo
```

If you want the remote descriptor to use a specific user name, set one of:

```bash
export MMR_GITHUB_USER="$(whoami)"
# or
export GITHUB_USER="$(whoami)"
```

The MVP remote is addressed as `github:<user>/mmr-store`. Tests can override the
file-backed remote location with `MMR_FAKE_REMOTE_DIR`.

## Link

Run first-run setup from the project directory:

```bash
mmr link --pretty
```

`link` is idempotent. It creates or reuses the local store, links the current
project, hydrates any existing remote payloads, imports available
Codex/Claude/Cursor roots, rebuilds search documents, applies redaction during
sync, and prints status JSON.

Provider roots checked by default:

- Codex: `$HOME/.codex`
- Claude: `$HOME/.claude`
- Cursor: `$HOME/.cursor`

Grok and Pi remain available through raw retrieval commands, but Memory Fabric
importers for those sources are not part of this MVP.

## Inspect State

Use status whenever setup or sync behavior is unclear:

```bash
mmr status --pretty
```

Key fields:

- `store.db_path`: local SQLite database path.
- `store.existed_before_command`: whether the database already existed before
  this `status` invocation opened/migrated it.
- `store.schema_version` and `store.expected_schema_version`: migration state.
- `project`: linked project identity, or `null` for an unlinked cwd.
- `status.sync_status`: `synced`, `pending`, `blocked`, `remote_unavailable`,
  `remote_missing`, or `unlinked`.
- `diagnostics.sources`: provider root availability and imported event counts.
- `diagnostics.privacy_filter`: optional privacy-filter coverage state.
- `diagnostics.remote`: remote availability and auth state.
- `diagnostics.summary_runner`: continuity brief provider availability for
  `summary` and the `remember` compatibility alias.
- `diagnostics.dream_runner`: mock or command runner availability.
- `diagnostics.actions`: concrete recovery commands or environment fixes.

## Add Local Notes

Notes are first-class events and are searchable, redacted, synced, summarized,
and dreamed through the same pipeline as imported agent events.

```bash
mmr note "Decision: keep the migration append-only and cover it with fixtures."
```

For multiline notes:

```bash
cat decision.md | mmr note
```

## Search

Structured JSON search:

```bash
mmr search "migration append-only" --pretty
```

POSIX-oriented line output is explicit:

```bash
mmr rg "migration append-only" --line
```

Default `rg` and `search` output remains JSON on stdout. Use `--pretty` for
indented JSON.

## Summarize

`summary` is the stateless continuity brief command. `remember` remains a
compatibility alias for existing scripts.

```bash
mmr summary --project "$(pwd)"
mmr summary all --project "$(pwd)"
mmr summary session <session-id> --project "$(pwd)"
```

The default summary backend is Cursor unless `MMR_DEFAULT_REMEMBER_AGENT` is
set. `mmr status --pretty` reports `diagnostics.summary_runner` so missing
`CURSOR_API_KEY`, the Cursor `agent` CLI, Gemini keys, or the Codex CLI are
visible before you run a summary.

`summary` and `remember` are stateless: they do not write active learned memory.
Use `mmr dream` for stateful assimilation.

## Dream

Preview learned-memory proposals without writing state:

```bash
mmr dream --dry-run --pretty
```

Queue a review-shaped response without active learned-memory writes:

```bash
mmr dream --review --pretty
```

Run the default mock runner and persist the dream audit:

```bash
mmr dream --pretty
```

Configure a local command runner:

```bash
export MMR_DEFAULT_DREAM_RUNNER=command
export MMR_DREAM_COMMAND="python ./dream_runner.py"
mmr dream --pretty
```

The command runner reads a dream request JSON object on stdin and writes
structured dream output JSON on stdout. Shared-safe evidence is the default:
deterministic local PII is redacted and secret-bearing events are omitted before
runner invocation.

## Sync

After adding notes or importing events, reconcile with the default remote:

```bash
mmr sync --pretty
```

Preview sync safety without remote writes:

```bash
mmr sync --dry-run --pretty
```

Sync uploads only redacted safe projections. Events with unresolved secrets,
tool-call/tool-result raw payloads, unknown raw event types, or degraded policy
coverage are blocked rather than uploaded.

## Raw Retrieval

Raw history browsing remains available and does not require the Memory Fabric
store:

```bash
mmr projects --pretty
mmr sessions --pretty
mmr messages --latest 5 --pretty
mmr export --pretty
```

`sessions`, `messages`, and `export` default to the current project when cwd
auto-discovery succeeds. Use `--all` for cross-project session/message queries.

## Recovery

Unlinked cwd:

```bash
mmr status --pretty
mmr link --pretty
```

Remote auth failure:

```bash
export MMR_GITHUB_USER="$(whoami)"
mmr status --pretty
```

Missing provider root:

```bash
mmr status --pretty
mmr --source codex import --project "$(pwd)" --source-root "$HOME/.codex"
```

Replace `codex` and the path with `claude`/`$HOME/.claude` or
`cursor`/`$HOME/.cursor` as needed.

Privacy-filter degraded:

```bash
mmr sync --dry-run --pretty
```

The optional `openai/privacy-filter` is not bundled. Deterministic secret and
coarse PII blocking still run before sync.

Summary provider unavailable:

```bash
mmr status --pretty
export MMR_DEFAULT_REMEMBER_AGENT=gemini
export GOOGLE_API_KEY="<key>"
mmr summary --agent gemini --project "$(pwd)"
```

For Cursor, set `CURSOR_API_KEY` and keep the `agent` CLI on `PATH`. For Codex,
install/authenticate the `codex` CLI and use `mmr summary --agent codex`.
`MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` changes which provider
`status` checks by default; `--agent` overrides it for a single summary run.

Blocked sync:

```bash
mmr redact scan --project "$(pwd)" --pretty
mmr redact explain <event-id> --pretty
```

Schema mismatch:

```bash
mmr status --pretty
```

Back up the database shown in `store.db_path`, update `mmr`, and rerun the
command so migrations can complete.

Dream command missing:

```bash
export MMR_DEFAULT_DREAM_RUNNER=command
export MMR_DREAM_COMMAND="python ./dream_runner.py"
mmr status --pretty
```

Use `MMR_DEFAULT_DREAM_RUNNER=mock` to return to the built-in local runner.
