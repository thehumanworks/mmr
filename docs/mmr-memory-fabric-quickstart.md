# mmr Memory Fabric Quickstart

Status: implemented through NHL-281. See
`docs/mmr-memory-fabric-release-gate.md` for final release evidence.

This guide covers the lean MVP flow from a blank non-Git directory to linked,
synced, searchable, and assimilation-ready local memory.

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

## Init

Run first-run setup from the project directory:

```bash
mmr init --pretty
```

`init` is idempotent. It creates or reuses the local store, links the current
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
  `summarize`.
- `diagnostics.dream_runner`: reports that no assimilation runner is required.
- `diagnostics.actions`: concrete recovery commands or environment fixes.

## Add Local Notes

Notes are first-class events and are searchable, redacted, synced, summarized,
and assimilated through the same pipeline as imported agent events.

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
mmr find "migration append-only" --pretty
```

POSIX-oriented line output is explicit:

```bash
mmr find "migration append-only" --format line
```

Default `find` output remains JSON on stdout. Use `--pretty` for indented JSON.

## Summarize

`summarize` is the stateless continuity brief command.

```bash
mmr summarize project --project "$(pwd)"
mmr --source codex summarize source
mmr summarize session <session-id> --project "$(pwd)"
```

The summary backend is an OpenAI-compatible Chat Completions API. Set
`OPENAI_API_KEY`, optionally set `OPENAI_BASE_URL` for OpenRouter or another
compatible proxy, and set `MMR_SUMMARISER_MODEL` or pass `--model`.
`mmr status --pretty` reports `diagnostics.summary_runner` so missing API
configuration is visible before you run a summary.

`summarize` and `assimilate` are stateless: they do not write active learned
memory. Use `mmr assimilate` to generate a prompt, runbook, output contract, and
cited evidence bundle for the calling AI agent.

## Assimilate

Generate the memory assimilation prompt and runbook:

```bash
mmr assimilate project --pretty
mmr --source codex assimilate source --pretty
```

`mmr assimilate` does not run an AI provider and does not write learned memory.
The JSON response includes:

- `system_prompt`: role and guardrails for the calling AI agent.
- `runbook`: ordered steps for deduplication, assimilation, and generalisation.
- `output_contract`: the shape the agent should return after analysis.
- `evidence.events`: shared-safe evidence with `mmr://event/...` refs.

Shared-safe evidence is the default: deterministic local PII is redacted and
secret-bearing events are omitted before being returned.

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
mmr list projects --pretty
mmr list sessions --pretty
mmr recall --pretty
mmr read project --pretty
mmr read session <session-id> --pretty
mmr --source codex read source --pretty
```

`list sessions`, `recall`, and `read project` default to the current project
when cwd auto-discovery succeeds. Use `--all` for cross-project session listing
or recall, and use `read source` for source-wide raw reads across projects.

## Agent Skill

Use the bundled skill when an agent needs operational guidance beyond `--help`:

```bash
mmr skill load
mmr skill install
mmr skill install --local
```

`skill load` prints the skill bundle to stdout. `skill install` replaces
`~/.agents/skills/mmr`; `--local` replaces `.agents/skills/mmr` under the
current project.

## Recovery

Unlinked cwd:

```bash
mmr status --pretty
mmr init --pretty
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
export OPENAI_API_KEY="<key>"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export MMR_SUMMARISER_MODEL="gpt-4o-mini"
mmr summarize project --project "$(pwd)"
```

Use `OPENAI_BASE_URL` to point at a compatible proxy such as OpenRouter, and
`MMR_SUMMARISER_MODEL` for the provider-specific model id. `--model` overrides
the environment for a single summary run.

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

`mmr assimilate` does not require `MMR_DREAM_COMMAND` or a configured local
runner.
