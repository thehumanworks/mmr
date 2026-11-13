# Cross-harness context eval: claude-paragraph-v2

- mmr: `mmr 0.2.0` (bbb65e5)
- agent: 2.1.323 (Claude Code), model `claude-sonnet-5-5`
- runs: 160 (2 per cell), 2026-11-13T00:56

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | signature | add-dep | redis-move | timeout | replay-delete | log-payload | backoff | all |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **0/32** |
| shipped-skill | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 1/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | **29/32** |
| skill | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | 0/2 | 0/2 | 2/2 | 0/2 | 2/2 | 0/2 | 1/2 | **19/32** |
| mcp | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 0/2 | 0/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | **26/32** |
| hook | 1/2 | 2/2 | 2/2 | 1/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 2/2 | 1/2 | 0/2 | 1/2 | 1/2 | 2/2 | **23/32** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/32 | 2 | 0 | 0 | 0 | 0 | 47k | 724 | 0.04 | 8 |
| shipped-skill | 32/32 | 3 | 2 | 0 | 12k | 0 | 58k | 881 | 0.06 | 9 |
| skill | 18/32 | 3 | 1 | 0 | 3k | 0 | 64k | 996 | 0.05 | 9 |
| mcp | 30/32 | 5 | 2 | 0 | 6k | 0 | 72k | 1k | 0.07 | 10 |
| hook | 26/32 | 2 | 1 | 0 | 4k | 0 | 57k | 877 | 0.05 | 9 |

## Rubric items missed

- **why-retries** (Claude Code session, main project path): duplicate charges: hook#2
- **redis** (Claude Code session run in a worktree under .claude/worktrees): reason: on-prem/ops has no Redis: hook#2
- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: skill#1, skill#2; Acme settlement batch: skill#1, skill#2
- **dashboard** (held out; sibling project (ledger-ui), Codex session): 2 retries: skill#1, shipped-skill#1, hook#1, mcp#1, skill#2, shipped-skill#2, hook#2, mcp#2; names the ledger-ui project: skill#1, shipped-skill#1, hook#1, mcp#1, skill#2, shipped-skill#2, hook#2, mcp#2
- **signature** (proactive; Codex session, a rule in the user's message after the preview cut): billing-svc imports deliver(): skill#1, mcp#1, skill#2, mcp#2
- **add-dep** (proactive; Codex session, the second rule in the user's message, past the preview cut): no new dependencies without asking: skill#1, shipped-skill#1, mcp#1, skill#2, mcp#2
- **redis-move** (proactive; Claude Code worktree session, the user's second message): on-prem installs have no Redis: hook#1
- **timeout** (proactive; Claude Code session, the reason is in the assistant's analysis): raised after duplicate charges: skill#1, hook#1, skill#2, hook#2; names Acme or billing: skill#1, hook#1, skill#2, hook#2
- **replay-delete** (proactive; Cursor session about dead letters, the user's second message): on-call runbook uses it: hook#1
- **log-payload** (proactive; Grok session reviewing logs, the user's second message): payloads hold customers' IBANs: skill#1, skill#2, hook#2
- **backoff** (proactive; Pi session about request rates, the user's second message): Acme's rate limit (5 req/s, or an hour's block): skill#1

## mmr commands used

- `mmr find`: 54
- `mcp__mmr__mmr_find`: 43
- `mmr read session`: 34
- `mcp__mmr__mmr_read_session`: 20
- `mmr --source read session`: 18
- `mmr list sessions`: 8
- `mcp__mmr__mmr_recall`: 2
- `mcp__mmr__mmr_context_project`: 2
- `for s in "codex f9471233-13a2-582c-b8af-`: 1
- `for s in "codex b88829fd-f7f9-5dc6-8575-`: 1
- `git diff; for s in "claude 8a3e4773-4aeb`: 1
- `for s in "claude 8a3e4773-4aeb-5975-908b`: 1
- `mmr read project`: 1
