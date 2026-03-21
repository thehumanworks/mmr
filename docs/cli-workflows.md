# CLI workflows and operational notes

This page captures user-facing behavior that is currently spread across clap help, `AGENTS.md`, and integration tests. It focuses on the workflows that changed most recently or have the most operational footguns: `remember`, `prompt`, `merge`, and `sync`.

## Project scoping cheat sheet

| Command | Default project scope | How to widen or override |
| --- | --- | --- |
| `mmr projects` | All projects | Optional `--source` / `MMR_DEFAULT_SOURCE` |
| `mmr sessions` | Auto-discovers the current working directory as the project when possible | Use `--project <path>` to force a project, `--all` to skip cwd scoping, or `MMR_AUTO_DISCOVER_PROJECT=0` to disable auto-discovery |
| `mmr messages` | Same cwd auto-discovery as `sessions` | Use `--project <path>`, `--session <id>`, `--all`, or `MMR_AUTO_DISCOVER_PROJECT=0` |
| `mmr export` | Current working directory only | Use `--project <path>` to export a specific project |
| `mmr remember` | Current working directory only | Use `--project <path>` to point at another project |
| `mmr prompt` | Current working directory only | Use `--project <path>` to point at another project |

Notes:

- `sessions` and `messages` fall back to global results only when cwd auto-discovery fails. If discovery succeeds but nothing matches, they return an empty result instead of widening the search.
- `remember` and `prompt` do not have an `--all` mode. If you run them from the wrong directory, they will use that directory unless you pass `--project`.
- In scripts, pass `--project` and the path as separate arguments. Avoid literal quoted values like `--project=\"/path\"`.

## Agent-backed workflows

### `--agent` chooses the backend

Both `remember` and `prompt` support these backends:

- `cursor`
- `codex`
- `gemini`

Resolution order:

1. explicit `--agent`
2. `MMR_DEFAULT_REMEMBER_AGENT`
3. built-in default: `cursor`

The same env default is used by both commands.

### Backend requirements and model behavior

| Backend | Required setup | Model behavior |
| --- | --- | --- |
| Cursor | `CURSOR_API_KEY` and the `agent` CLI on `PATH` | Defaults to `composer-2-fast`; `--model` is forwarded to the Cursor agent |
| Gemini | `GOOGLE_API_KEY` or `GEMINI_API_KEY`; optional `GEMINI_API_BASE_URL` override | Defaults to `gemini-3.1-flash-lite-preview`; `--model` is honored |
| Codex | Codex CLI auth as configured for the local Codex client | Uses the built-in default model `gpt-5.4-mini` with medium reasoning effort; `--model` is currently ignored by the Codex backend |

### `remember`: continuity briefs from session transcripts

`remember` reads session transcripts for one project and asks the selected backend to produce a continuity brief.

Selectors:

- `mmr remember` -> latest matching session only
- `mmr remember all` -> all matching sessions, newest session first
- `mmr remember session <session-id>` -> one specific session

Important behavior:

- `--source codex|claude` filters which transcripts are included.
- Output defaults to markdown (`-O md` / `--output-format md`).
- `-O json` returns a JSON object with `agent` and `text`.
- The prompt sent to the backend is one-shot only; it does not resume prior backend conversations.
- Tool messages longer than 2000 characters are truncated before being sent to the backend.

`--instructions` has a strong override contract:

- the base "Memory Agent" identity and input-format section are always preserved
- the default output instruction is fully replaced, not appended to

Example:

```bash
mmr remember --project /path/to/proj
mmr remember all --project /path/to/proj --agent gemini -O json
mmr remember session sess-123 --project /path/to/proj --source codex
mmr remember --project /path/to/proj --instructions "Return only a single keyword."
```

### `prompt`: optimized prompts for Claude or Codex

`prompt` is different from `remember` in one important way:

- `--agent` selects the backend that does the optimization
- `--target` selects the agent the output prompt is written for (`claude` or `codex`)

Other verified behavior:

- `--target` is required.
- Output is raw prompt text on `stdout`, not JSON.
- After generating the prompt, `mmr` tries to copy it to the system clipboard. Clipboard failure is intentionally non-fatal.
- When session history exists for the project, `prompt` uses those transcripts as context.
- If no session history is available, it falls back to codebase context by running a bounded `rg` file listing plus a few keyword searches under the `--project` directory.
- If neither session history nor codebase context is available, the backend sees the query only.

Examples:

```bash
mmr prompt "implement user authentication" --target claude --project /path/to/proj
mmr prompt "fix flaky merge test" --target codex --agent cursor --project /path/to/proj
mmr prompt "add sync docs" --target claude --agent gemini --model gemini-3.1-flash-lite-preview
```

## Merge runbook

Use `merge --dry-run` first. The dry-run path uses the same resolution logic as a real merge but does not mutate history files.

Recommended flow:

1. Identify the source and destination sessions or agents.
2. Run a dry-run and inspect the JSON response.
3. If you want a backup of the exact input files the planner read, add `--zip-output <path>`.
4. Run the real merge only after the dry-run plan looks correct.

Examples:

```bash
mmr merge --from-session sess-claude-1 --to-session sess-codex-1 --dry-run
mmr merge --from-session sess-claude-1 --to-session sess-codex-1 --dry-run --zip-output /tmp/mmr-merge-inputs.zip
mmr merge --from-agent claude --to-agent codex --project /path/to/proj
```

Key constraints:

- `--zip-output` requires `--dry-run`.
- `merge` does not use the global `--source` flag. Use `--from-agent` / `--to-agent` when you need to disambiguate sources.
- Session-to-session merges require both `--from-session` and `--to-session`.
- Agent-to-agent merges require different agents.
- Dry-run responses include `resolved_history_files`, per-session `source_files`, `action`, and the resolved `target_file`.

## Sync runbook

`sync` mirrors local Claude/Codex history files to Cloudflare R2-compatible storage.

### What gets synced

Only `.jsonl` conversation files under these locations are considered:

- `~/.claude/projects`
- `~/.codex/sessions`
- `~/.codex/archived_sessions`

### Initial setup

Run:

```bash
mmr sync init
```

This writes `~/.config/mmr/sync.toml` with:

- storage provider `r2`
- endpoint, bucket, access key, and secret key
- quiet hours defaulting to `03:00`-`09:00` in `Europe/London`
- both Claude and Codex sources enabled

Environment overrides are available for the storage credentials:

- `MMR_SYNC_ENDPOINT`
- `MMR_SYNC_BUCKET`
- `MMR_SYNC_ACCESS_KEY_ID`
- `MMR_SYNC_SECRET_ACCESS_KEY`

### Push, pull, and status semantics

- `mmr sync push` uploads new or modified local history files and updates the local manifest after a successful upload.
- `mmr sync push --dry-run` reports what would be uploaded without writing.
- `mmr sync pull` is non-destructive: existing local files are never overwritten.
- Diverged local files are reported as conflicts during pull or status.
- `mmr sync status` summarizes local-only, modified, synced, and remote-only files using the local and remote manifests.

Examples:

```bash
mmr sync status
mmr sync push --dry-run
mmr sync pull --dry-run
mmr sync push
```

### Background daemon installation

Supported platforms:

- macOS -> LaunchAgent
- Linux -> systemd user timer

Commands:

```bash
mmr sync install
mmr sync install --interval 30
mmr sync uninstall
```

Operational notes:

- `install` requires a valid sync config first.
- The wrapper script runs `mmr sync push`.
- Quiet hours come from the sync config and are enforced before each scheduled push.
- On Linux, units are written under `~/.config/systemd/user`.
- On macOS, the plist is written under `~/Library/LaunchAgents`.

## Troubleshooting and common pitfalls

- `remember` or `prompt` fails immediately: verify the backend-specific auth above, then check that `--project` points at the right project.
- `prompt` prints plain text instead of JSON: this is expected behavior.
- `remember` returned markdown when you expected JSON: use `-O json`.
- `sessions` or `messages` seems too narrow: cwd auto-discovery may be active; retry with `--all`.
- `merge` rejects `--source`: use `--from-agent` / `--to-agent` instead.
- `sync` says it is not configured: run `mmr sync init` and confirm `~/.config/mmr/sync.toml` exists.
