# mmr

 `mmr` is a Rust CLI for browsing local AI coding history from Claude, Codex, and Cursor.
 It reads transcript files directly from disk, normalizes them into a shared in-memory model,
 and returns machine-readable JSON on stdout.

 ## What it covers

 - `projects`: aggregate projects across local history sources
 - `sessions`: list sessions with previews and message counts
 - `messages`: inspect message streams with pagination metadata
 - `export`: emit all messages for one project as chronological JSON
 - `remember`: generate a stateless continuity brief from prior sessions

 ## Where data comes from

 `mmr` reads transcript data from the current user's home directory unless
 `SIMPLEMMR_HOME` is set.

 | Source | Locations read |
 | --- | --- |
 | Codex | `~/.codex/sessions/**/*.jsonl`, `~/.codex/archived_sessions/**/*.jsonl` |
 | Claude | `~/.claude/projects/**/*.jsonl` and nested `subagents/*.jsonl` files |
 | Cursor | `~/.cursor/projects/*/agent-transcripts/*/*.jsonl` |

 Parsing is defensive: malformed JSONL lines are skipped and valid records continue to load.

 ## Quick start

 ```bash
 cargo run -- projects
 cargo run -- sessions
 cargo run -- messages
 cargo run -- export --project /Users/test/codex-proj
 cargo run -- remember --project /Users/test/codex-proj
 ```

 All read commands support `--source claude|codex|cursor`. Omitting `--source` searches all
 sources unless `MMR_DEFAULT_SOURCE` supplies a default.

 ## Common workflows

 ### Inspect the current project's recent sessions

 ```bash
 cargo run -- sessions
 ```

 When cwd auto-discovery succeeds, `sessions` scopes to the current directory by default.
 Use `--all` to search across every project instead.

 ### Inspect messages for one session

 ```bash
 cargo run -- messages --session sess-123
 ```

 `messages --session <id>` searches all projects when `--project` is omitted, because the
 session ID is already the primary selector. If `--source` is also omitted, `mmr` prints a
 narrowing hint on stderr and still returns JSON on stdout.

 ### Export one project's full message history

 ```bash
 cargo run -- export --project /Users/test/codex-proj
 ```

 `export` always returns `ApiMessagesResponse`, the same envelope used by `messages`.
 Without `--project`, it infers the project from the current working directory.

 ### Generate a continuity brief

 ```bash
 cargo run -- remember --project /Users/test/codex-proj
 cargo run -- remember all --project /Users/test/codex-proj
 cargo run -- remember session sess-123 --project /Users/test/codex-proj
 ```

 `remember` defaults to Markdown output. For automation, request JSON explicitly:

 ```bash
 cargo run -- remember --project /Users/test/codex-proj --output-format json
 ```

 ## CLI behavior that matters in automation

 ### stdout vs stderr

 - JSON responses are written to stdout.
 - Hints and diagnostics are written to stderr.
 - `remember --output-format md` writes Markdown to stdout by design.

 ### Source defaults

 `MMR_DEFAULT_SOURCE=codex|claude|cursor` supplies the default source when `--source` is
 omitted. Empty or invalid values are treated as unset.

 ### CWD project auto-discovery

 `sessions` and `messages` default to the current project when:

 - `--project` is omitted
 - `--all` is omitted
 - `MMR_AUTO_DISCOVER_PROJECT` is unset or `1`

 If cwd resolution fails, the CLI falls back to the historical global behavior. If cwd
 resolution succeeds but no records match, the command returns an empty result instead of
 widening the query.

 `export` also infers the project from cwd when `--project` is omitted:

 - Codex matches the canonical path directly
 - Claude and Cursor match the slash-to-hyphen form with a leading hyphen

 ### Messages pagination

 `messages` returns pagination metadata:

 - `next_page`
 - `next_offset`
 - `next_command`

 When sorting by ascending timestamp, paging is computed from the newest window and the
 returned page is then re-ordered chronologically. This preserves the repo's historical
 contract for recent-message inspection.

 ## Remember backends

 `remember` supports three backends:

 - `cursor` (default): uses the local `agent` CLI and defaults to model `composer-2-fast`
 - `codex`: uses the Codex app server SDK with fixed model `gpt-5.4-mini` and medium reasoning
 - `gemini`: uses the Gemini API and honors `GOOGLE_API_KEY` or `GEMINI_API_KEY`

 `MMR_DEFAULT_REMEMBER_AGENT=cursor|codex|gemini` supplies the default backend when
 `--agent` is omitted.

 `--instructions` replaces the default output-format section of the remember system prompt
 while preserving the base "Memory Agent" identity and transcript input description.

 ## Setup and local development

 ```bash
 cargo fmt
 cargo test
 cargo test --test cli_benchmark -- --ignored --nocapture
 cargo clippy --all-targets --all-features -- -D warnings
 cargo build --release
 ```

 The repository uses Rust 2024 edition. If your local `cargo` is too old to understand
 `edition = "2024"`, install or select a current stable Rust toolchain before building.

 ## Troubleshooting

 ### No results when you expected project-local history

 Check whether the current directory matches the project naming used by each source:

 - Codex stores the canonical path directly
 - Claude and Cursor store a hyphenated project name and may also carry the original cwd path

 If you are scripting `mmr`, pass `--project` and the value as separate arguments rather than
 embedding shell quotes into a single argument.

 ### `messages --session` looks wider than `messages`

 That is expected. Session lookups intentionally bypass cwd project auto-discovery unless you
 also pass `--project`.

 ### `remember` output is Markdown instead of JSON

 That is the default. Use `--output-format json` if another tool will parse the response.

 ### Build fails with an edition error

 Your Rust toolchain is likely outdated. Upgrade to a recent stable release and rerun the
 build commands.

 ## More repository docs

 - `AGENTS.md`: contributor-facing architecture notes and command reference
 - `adrs/002-cwd-scoped-defaults.md`: rationale for cwd-based defaults
 - `docs/references/session-lookup-invariants.md`: session lookup behavior contract
