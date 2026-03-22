# CLI Workflows: Prompt Optimization and Cloud Sync

This page covers two `mmr` workflows that are implemented in the CLI but were not previously documented in a durable, operator-facing guide:

- `mmr prompt` for generating a better task prompt for another coding agent
- `mmr sync` for backing up local Claude/Codex history to cloud storage

Use this page alongside `AGENTS.md`, which remains the repo's contributor-oriented index.

## `mmr prompt`

### What it does

`mmr prompt` asks one backend agent to generate an optimized prompt for a target coding agent.

- `--target` selects the prompt style you want back: `claude` or `codex`
- `--agent` selects the backend that generates that prompt: `cursor`, `codex`, or `gemini`
- `--project` selects which project's history to mine for context; if omitted, `mmr` uses the current working directory path
- global `--source` still applies and limits which source histories are searched for prior sessions

This is a text-producing command, not a JSON API command. It prints the optimized prompt directly to `stdout`.

### Output contract

`mmr prompt` intentionally returns raw prompt text rather than a JSON envelope.

- `stdout`: the optimized prompt text only
- `stderr`: human-facing errors
- clipboard copy: best effort only; clipboard failures are ignored

That means `--pretty` does not change `prompt` output in the same way it does for JSON-returning commands.

### Context selection

`mmr prompt` prefers prior session history over ad hoc code search.

1. It loads all matching sessions for the requested project and optional `--source` filter.
2. If matching sessions exist, it formats those transcripts and sends them to the backend.
3. If no sessions are found, it falls back to lightweight codebase context from the project path:
   - up to 50 file paths from `rg --files`
   - up to 3 search matches for each of up to 5 query keywords

The fallback is only used when transcript loading fails or returns no sessions; it is not combined with session history.

Tool-role messages in transcript context are truncated after 2000 characters before being sent to the optimizer backend.

### Examples

Generate a Claude-oriented prompt from project history:

```bash
mmr prompt "implement user authentication" --target claude --project /Users/test/proj
```

Generate a Codex-oriented prompt with Gemini as the backend:

```bash
mmr prompt "fix authentication bug" --target codex --agent gemini --project /Users/test/proj
```

Limit context gathering to Codex sessions only:

```bash
mmr --source codex prompt "add benchmark coverage" --target codex --project /Users/test/proj
```

Use the current working directory as the project automatically:

```bash
cd /Users/test/proj
mmr prompt "add retry logic" --target claude
```

### Constraints and pitfalls

- `--target` is required.
- The query is a required positional argument.
- `MMR_DEFAULT_REMEMBER_AGENT` also supplies the default backend for `prompt --agent`.
- If you omit `--project`, the command uses the current working directory path as-is; make sure it matches how the project is recorded in history.
- If the selected backend is not configured, the command fails on `stderr` rather than returning a partial prompt.

### Troubleshooting

| Problem | What to check |
| --- | --- |
| `prompt` fails before contacting the backend | Confirm the backend-specific credentials and tooling are available (`GOOGLE_API_KEY`/`GEMINI_API_KEY`, `CURSOR_API_KEY` + `agent`, or Codex CLI auth). |
| Output is empty or lacks repo context | Verify `--project` points at the same project identifier stored in history, or use global `--source` to narrow mixed-source projects. |
| Clipboard did not update | The command still succeeds if clipboard access is unavailable; copy the returned `stdout` manually. |

## `mmr sync`

### What it does

`mmr sync` manages cloud backup for local Claude and Codex conversation history.

Subcommands:

- `mmr sync init`
- `mmr sync push [--dry-run]`
- `mmr sync pull [--dry-run]`
- `mmr sync status`
- `mmr sync install [--interval <minutes>]`
- `mmr sync uninstall`

### Local files and remote layout

`mmr sync` reads conversation history under the current `HOME` directory:

- Claude: `~/.claude/projects/**/*.jsonl`
- Codex: `~/.codex/sessions/**/*.jsonl`
- Codex archived: `~/.codex/archived_sessions/**/*.jsonl`

It stores sync state in `~/.config/mmr/`:

- `sync.toml`: user config
- `sync-manifest.json`: local manifest of synced files
- `sync.lock`: concurrency guard for push/pull
- `sync-daemon.sh`: daemon wrapper script created by `install`

Remote objects use an `mmr/` prefix:

- history files: `mmr/<relative path under HOME>`
- remote manifest: `mmr/manifest.json`

### Setup

Run interactive setup first:

```bash
mmr sync init
```

`init` prompts for:

- Cloudflare R2 endpoint
- bucket name
- access key ID
- secret access key

The saved config defaults to:

- provider: `r2`
- region: `auto`
- quiet hours: `03:00` to `09:00`
- quiet timezone: `Europe/London`
- daemon interval: `15` minutes
- enabled sources: both Claude and Codex

On Unix, the config file is written with `0600` permissions.

### Config example

```toml
[storage]
provider = "r2"
endpoint = "https://<account_id>.r2.cloudflarestorage.com"
bucket = "my-history-bucket"
access_key_id = "AKIA..."
secret_access_key = "..."
region = "auto"

[sync]
quiet_start = "03:00"
quiet_end = "09:00"
quiet_timezone = "Europe/London"
interval_minutes = 15

[sources]
claude = true
codex = true
```

The following environment variables override the stored storage settings at runtime:

- `MMR_SYNC_ENDPOINT`
- `MMR_SYNC_BUCKET`
- `MMR_SYNC_ACCESS_KEY_ID`
- `MMR_SYNC_SECRET_ACCESS_KEY`

### Push behavior

`mmr sync push` uploads only new or modified local `.jsonl` files compared with the local manifest.

```bash
mmr sync push --dry-run
mmr sync push
```

Behavior:

- unchanged files are skipped
- new/modified files are uploaded to `mmr/<relative path>`
- after a real push, `mmr/manifest.json` is uploaded and the local manifest is updated
- `--dry-run` reports planned uploads without mutating local or remote state

`push` returns machine-readable JSON on `stdout`.

### Pull behavior

`mmr sync pull` is intentionally non-destructive.

```bash
mmr sync pull --dry-run
mmr sync pull
```

Behavior:

- missing local files are downloaded from the remote manifest
- existing local files are never overwritten
- if a local file exists but its hash differs from the remote manifest, the file is kept locally and reported as a conflict
- `--dry-run` reports planned downloads without writing files

`pull` also returns machine-readable JSON on `stdout`.

### Status behavior

`mmr sync status` compares:

- local files vs the local manifest (`local new`, `local modified`, `synced`)
- remote manifest vs the local filesystem (`remote only`, `diverged`)

```bash
mmr sync status
```

The response includes a summary line plus optional conflict details, and it returns JSON on `stdout`.

### Daemon install and quiet hours

`install` configures a background wrapper that runs `mmr sync push` on a timer:

- macOS: LaunchAgent
- Linux: systemd user service + timer

```bash
mmr sync install
mmr sync install --interval 30
mmr sync uninstall
```

Important constraints:

- `install` requires a valid sync config first
- daemon install/uninstall is only supported on macOS and Linux
- quiet hours are enforced by the generated wrapper script before it runs `mmr sync push`
- manual `mmr sync push` and `mmr sync pull` do not check quiet hours

`init`, `install`, and `uninstall` return human-readable text rather than JSON.

### Locking and recovery

`push` and `pull` acquire `~/.config/mmr/sync.lock` to prevent concurrent sync runs.

- an existing lock newer than 30 minutes causes the command to fail
- a lock older than 30 minutes is treated as stale and removed automatically

If repeated sync runs fail with a lock error, inspect the lock path and confirm no active sync process is still running.

### Common pitfalls

| Problem | What to check |
| --- | --- |
| `sync not configured` | Run `mmr sync init` or create `~/.config/mmr/sync.toml`. |
| `push` uploads fewer files than expected | Check `[sources]` in `sync.toml`; only enabled sources are scanned. Only `.jsonl` files are synced. |
| `pull` did not update an existing local file | This is expected. Pull is non-destructive and keeps local files when hashes differ. |
| `install` fails on Linux | Verify `systemctl --user` is available for the current user session. |
| `install` succeeds but nothing runs during quiet hours | This is expected; the wrapper exits early during the configured quiet window. |
