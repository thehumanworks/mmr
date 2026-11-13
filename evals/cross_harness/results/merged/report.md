# Cross-harness context eval: merged

- mmr: `mmr 0.2.0` (10383c3)
- agent: Claude Code 2.1.323 (Claude Code), model `claude-sonnet-5-5`
- runs: 126 (2 per cell), 2026-11-12T23:57

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | all |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **0/18** |
| path | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **0/18** |
| shipped-skill | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **14/18** |
| skill | 1/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **13/18** |
| told | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **14/18** |
| mcp | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | **14/18** |
| hook | 1/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | **15/18** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/18 | 2 | 0 | 0 | 0 | 1 | 46k | 870 | 0.04 | 9 |
| path | 0/18 | 2 | 0 | 0 | 0 | 0 | 47k | 956 | 0.04 | 9 |
| shipped-skill | 15/18 | 3 | 2 | 0 | 8k | 0 | 67k | 1k | 0.06 | 11 |
| skill | 14/18 | 3 | 1 | 0 | 6k | 0 | 66k | 1k | 0.06 | 11 |
| told | 14/18 | 3 | 3 | 0 | 8k | 0 | 68k | 1k | 0.05 | 10 |
| mcp | 16/18 | 5 | 2 | 0 | 7k | 0 | 69k | 1k | 0.07 | 10 |
| hook | 16/18 | 2 | 1 | 1 | 4k | 0 | 55k | 1k | 0.06 | 10 |

## Rubric items missed

- **why-retries** (Claude Code session, main project path): duplicate charges: path#1, skill#1, path#2, hook#2; names Acme or billing: path#1, path#2; partner keeps processing after timeout: path#2
- **resume** (Pi session (newest before this one)): migration 0007 next: path#1, path#2; 72-hour key cleanup: path#1, path#2
- **constraints** (Codex session (current Codex format), user message): keep deliver() signature: path#1, path#2; because billing-svc imports it: path#1, path#2; no new dependencies without asking: path#1, path#2
- **redis** (Claude Code session run in a worktree under .claude/worktrees): reason: on-prem/ops has no Redis: path#1, path#2
- **staging-url** (Grok session run from the scripts/ subdirectory; a detail mentioned once): exact staging URL: path#1, path#2
- **retry-history** (synthesis across an archived Codex session, a Claude Code session and a Cursor session): cut after Acme/billing duplicate charges: path#1, path#2; jitter after Acme's outage: path#1, path#2
- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: path#1, shipped-skill#1, skill#1, told#1, mcp#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2; Acme settlement batch: path#1, shipped-skill#1, skill#1, told#1, mcp#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2
- **flaky** (held out; Cursor session): test_store: path#1, path#2; cause: CI jobs share one DB file (database is locked): path#1, path#2
- **dashboard** (held out; sibling project (ledger-ui), Codex session): 2 retries: path#1, skill#1, hook#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2, hook#2; names the ledger-ui project: path#1, shipped-skill#1, skill#1, told#1, mcp#1, hook#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2, hook#2

## mmr commands used

- `mmr find`: 35
- `mmr read session`: 26
- `mcp__mmr__mmr_find`: 23
- `mmr --source read session`: 13
- `mmr list sessions`: 9
- `mcp__mmr__mmr_read_session`: 8
- `mmr --help>&1 | head -40`: 7
- `mmr recall`: 3
- `for s in "claude 8a3e4773-4aeb-5975-908b`: 2
- `for s in "codex f9471233-13a2-582c-b8af-`: 2
- `mcp__mmr__mmr_recall`: 2
- `mcp__mmr__mmr_context_project`: 2
- `mmr read project`: 2
- `mmr --help>&1 | head -30`: 1
- `git diff; for s in "claude read session `: 1
- `mmr --help>&1 | head -30; cd /tmp/clau`: 1
- `for s in "grok 86e28a15-5548-53f9-86b0-a`: 1
- `mmr --help; cd /tmp/claude-0/mmr-eval-wo`: 1
- `for s in "codex b88829fd-f7f9-5dc6-8575-`: 1
- `git log -G"MAX_RETRIES = " --format='%h `: 1
- `mmr search redis`: 1
- `mmr --help; ls /tmp/claude-0/mmr-eval-wo`: 1
- `grep -rniE "flak|xfail|skip|retry|rerun|`: 1
- `mmr search`: 1
