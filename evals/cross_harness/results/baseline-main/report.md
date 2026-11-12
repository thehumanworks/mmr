# Cross-harness context eval: baseline-main

- mmr: `mmr 0.2.0` (83f0fb5)
- agent: Claude Code 2.1.323 (Claude Code), model `claude-sonnet-5-5`
- runs: 72 (2 per cell), 2026-11-12T23:10

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | all |
|---|---|---|---|---|---|---|---|
| none | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 1/2 | **1/12** |
| path | 0/2 | 0/2 | 0/2 | 0/2 | 1/2 | 1/2 | **2/12** |
| shipped-skill | 0/2 | 0/2 | 0/2 | 1/2 | 0/2 | 1/2 | **2/12** |
| skill | 0/2 | 2/2 | 0/2 | 0/2 | 0/2 | 1/2 | **3/12** |
| told | 2/2 | 2/2 | 0/2 | 0/2 | 1/2 | 1/2 | **6/12** |
| mcp | 0/2 | 2/2 | 0/2 | 2/2 | 2/2 | 1/2 | **7/12** |

## Cost per run (median over all tasks)

| condition | runs using mmr | tool calls | mmr calls | mmr errors | mmr output chars | raw history reads | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/12 | 2 | 0 | 0 | 0 | 8 | 49k | 1k | 0.05 | 12 |
| path | 0/12 | 2 | 0 | 0 | 0 | 8 | 48k | 976 | 0.04 | 9 |
| shipped-skill | 0/12 | 3 | 0 | 0 | 0 | 7 | 51k | 1k | 0.05 | 10 |
| skill | 8/12 | 5 | 3 | 7 | 7k | 5 | 121k | 2k | 0.08 | 18 |
| told | 12/12 | 6 | 5 | 7 | 23k | 0 | 122k | 2k | 0.11 | 17 |
| mcp | 8/12 | 7 | 3 | 24 | 7k | 2 | 73k | 2k | 0.09 | 14 |

## Rubric items missed

- **why-retries** (Claude Code session, main project path): names Acme or billing: path#1, shipped-skill#1, skill#1, mcp#1, path#2, shipped-skill#2, skill#2, mcp#2; duplicate charges: shipped-skill#1, skill#1, mcp#1, path#2, shipped-skill#2, skill#2, mcp#2; partner keeps processing after timeout: skill#1
- **resume** (Pi session (newest before this one)): migration 0007 next: path#1; 72-hour key cleanup: path#1, shipped-skill#1, path#2, shipped-skill#2
- **constraints** (Codex session (current Codex format), user message): keep deliver() signature: path#1, shipped-skill#1, skill#1, told#1, mcp#1, shipped-skill#2, skill#2, told#2, mcp#2; because billing-svc imports it: path#1, shipped-skill#1, skill#1, told#1, mcp#1, path#2, shipped-skill#2, skill#2, told#2, mcp#2; no new dependencies without asking: path#1, shipped-skill#1, mcp#1, path#2, shipped-skill#2, skill#2, mcp#2
- **redis** (Claude Code session run in a worktree under .claude/worktrees): reason: on-prem/ops has no Redis: path#1, shipped-skill#1, skill#1, told#1, path#2, skill#2, told#2; mentions Redis was tried: shipped-skill#1, skill#2
- **staging-url** (Grok session run from the scripts/ subdirectory; a detail mentioned once): exact staging URL: shipped-skill#1, skill#1, told#1, path#2, shipped-skill#2, skill#2
- **retry-history** (synthesis across an archived Codex session, a Claude Code session and a Cursor session): cut because of duplicate charges: shipped-skill#1, skill#2; jitter reason: thundering herd: shipped-skill#1, path#2, skill#2, told#2, mcp#2

## mmr commands used

- `mcp__mmr__mmr_find`: 19
- `mmr list sessions`: 17
- `mmr find`: 16
- `mcp__mmr__mmr_read_session`: 16
- `mmr read project`: 9
- `mmr --source read session`: 8
- `mmr --help>&1 | head -40`: 7
- `mmr read session`: 7
- `mcp__mmr__mmr_read_project`: 7
- `mcp__mmr__mmr_list_sessions`: 6
- `for s in "pi 64cba498-6ae0-5e58-85ec-5f9`: 5
- `mmr list projects`: 5
- `mmr recall`: 4
- `mcp__mmr__mmr_list_projects`: 4
- `mmr list`: 3
- `mmr context project`: 3
- `mmr --help>&1 | head -30`: 2
- `mcp__mmr__mmr_recall`: 2
- `mmr --help>&1 | head -40; echo ---; gr`: 1
- `mcp__mmr__mmr_context_project`: 1
- `mmr read`: 1
- `P=$PWD; for s in "claude 8a3e4773-4aeb-5`: 1
- `for s in "claude 8a3e4773-4aeb-5975-908b`: 1
- `mmr --help | head -40; git log --stat`: 1
- `mmr --help>&1 | head -30; echo ---; gr`: 1
- `mcp__mmr__mmr_status`: 1
