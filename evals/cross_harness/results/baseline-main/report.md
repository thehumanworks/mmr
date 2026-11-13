# Cross-harness context eval: baseline-main

- mmr: `mmr 0.2.0` (83f0fb5 (main))
- agent: Claude Code 2.1.323 (Claude Code), model `claude-sonnet-5-5`
- runs: 126 (2 per cell), 2026-11-12T23:32

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | all |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **0/18** |
| path | 0/2 | 0/2 | 0/2 | 0/2 | 1/2 | 0/2 | 0/2 | 0/2 | 0/2 | **1/18** |
| shipped-skill | 0/2 | 0/2 | 0/2 | 1/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **1/18** |
| skill | 0/2 | 2/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **2/18** |
| told | 2/2 | 2/2 | 0/2 | 0/2 | 1/2 | 0/2 | 0/2 | 0/2 | 0/2 | **5/18** |
| mcp | 0/2 | 2/2 | 0/2 | 2/2 | 2/2 | 0/2 | 0/2 | 0/2 | 0/2 | **6/18** |
| hook | 2/2 | 2/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **4/18** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/18 | 2 | 0 | 0 | 0 | 1 | 48k | 968 | 0.04 | 9 |
| path | 0/18 | 2 | 0 | 0 | 0 | 0 | 48k | 930 | 0.04 | 8 |
| shipped-skill | 0/18 | 3 | 0 | 0 | 0 | 1 | 48k | 842 | 0.04 | 8 |
| skill | 8/18 | 3 | 0 | 7 | 0 | 0 | 66k | 1k | 0.05 | 11 |
| told | 14/18 | 6 | 5 | 10 | 20k | 0 | 114k | 2k | 0.10 | 16 |
| mcp | 9/18 | 3 | 1 | 24 | 3k | 0 | 66k | 1k | 0.05 | 12 |
| hook | 14/18 | 3 | 1 | 1 | 12k | 0 | 53k | 1k | 0.07 | 11 |

## Rubric items missed

- **why-retries** (Claude Code session, main project path): names Acme or billing: path#1, shipped-skill#1, skill#1, mcp#1, path#2, shipped-skill#2, skill#2, mcp#2; duplicate charges: shipped-skill#1, skill#1, mcp#1, path#2, shipped-skill#2, skill#2, mcp#2; partner keeps processing after timeout: skill#1
- **resume** (Pi session (newest before this one)): migration 0007 next: path#1; 72-hour key cleanup: path#1, shipped-skill#1, path#2, shipped-skill#2
- **constraints** (Codex session (current Codex format), user message): keep deliver() signature: path#1, shipped-skill#1, skill#1, told#1, mcp#1, shipped-skill#2, skill#2, told#2, mcp#2, hook#1, hook#2; because billing-svc imports it: path#1, shipped-skill#1, skill#1, told#1, mcp#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2, hook#1, hook#2; no new dependencies without asking: path#1, shipped-skill#1, mcp#1, path#2, shipped-skill#2, skill#2, mcp#2, hook#1, hook#2
- **redis** (Claude Code session run in a worktree under .claude/worktrees): reason: on-prem/ops has no Redis: path#1, shipped-skill#1, skill#1, told#1, path#2, skill#2, told#2, hook#1, hook#2; mentions Redis was tried: shipped-skill#1, skill#2
- **staging-url** (Grok session run from the scripts/ subdirectory; a detail mentioned once): exact staging URL: shipped-skill#1, skill#1, told#1, path#2, shipped-skill#2, skill#2, hook#1, hook#2
- **retry-history** (synthesis across an archived Codex session, a Claude Code session and a Cursor session): cut after Acme/billing duplicate charges: path#1, shipped-skill#1, skill#1, mcp#1, path#2, shipped-skill#2, skill#2, mcp#2; jitter after Acme's outage: path#1, shipped-skill#1, skill#1, told#1, mcp#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2, hook#1, hook#2
- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: path#1, shipped-skill#1, skill#1, told#1, mcp#1, hook#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2, hook#2; Acme settlement batch: path#1, shipped-skill#1, skill#1, told#1, mcp#1, hook#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2, hook#2
- **flaky** (held out; Cursor session): test_store: path#1, shipped-skill#1, skill#1, mcp#1, told#1, hook#1, path#2, shipped-skill#2, skill#2, hook#2; cause: CI jobs share one DB file (database is locked): path#1, shipped-skill#1, skill#1, mcp#1, told#1, hook#1, path#2, shipped-skill#2, skill#2, mcp#2, told#2, hook#2
- **dashboard** (held out; sibling project (ledger-ui), Codex session): 2 retries: path#1, skill#1, told#1, mcp#1, hook#1, path#2, shipped-skill#2, told#2, mcp#2, hook#2; names the ledger-ui project: path#1, shipped-skill#1, skill#1, told#1, mcp#1, hook#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2, hook#2

## mmr commands used

- `mmr read session`: 29
- `mmr find`: 20
- `mcp__mmr__mmr_find`: 19
- `mmr list sessions`: 18
- `mcp__mmr__mmr_read_session`: 16
- `mmr read project`: 12
- `mmr --source read session`: 8
- `mmr --help>&1 | head -40`: 7
- `mcp__mmr__mmr_read_project`: 7
- `mcp__mmr__mmr_list_sessions`: 6
- `for s in "pi 64cba498-6ae0-5e58-85ec-5f9`: 5
- `mmr list projects`: 5
- `mmr recall`: 4
- `mmr list`: 4
- `mcp__mmr__mmr_list_projects`: 4
- `mmr context project`: 3
- `mcp__mmr__mmr_recall`: 3
- `mmr --help>&1 | head -30`: 2
- `mmr read`: 2
- `mmr --help>&1 | head -40; echo ---; gr`: 1
- `mcp__mmr__mmr_context_project`: 1
- `P=$PWD; for s in "claude 8a3e4773-4aeb-5`: 1
- `for s in "claude 8a3e4773-4aeb-5975-908b`: 1
- `mmr --help | head -40; git log --stat`: 1
- `mmr --help>&1 | head -30; echo ---; gr`: 1
- `mcp__mmr__mmr_status`: 1
- `grep -rniE "flaky|flake|xfail|skip|retry`: 1
- `cd /tmp/claude-0/mmr-eval-work/told__fla`: 1
- `mmr search redis`: 1
