# Cross-harness context eval: claude-paragraph-v1

- mmr: `mmr 0.2.0` (6564fa3)
- agent: 2.1.323 (Claude Code), model `claude-sonnet-5-5`
- runs: 156 (2 per cell), 2026-11-13T00:47

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | signature | add-dep | redis-move | timeout | replay-delete | log-payload | backoff | all |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | – | – | – | **0/26** |
| shipped-skill | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 2/2 | 2/2 | 0/2 | – | – | – | **22/26** |
| skill | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | – | – | – | **14/26** |
| mcp | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | 0/2 | 0/2 | 1/2 | 0/2 | – | – | – | **15/26** |
| hook | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 2/2 | 2/2 | 0/2 | – | – | – | **22/26** |
| instructions | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 2/2 | 2/2 | 0/2 | – | – | – | **22/26** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/26 | 2 | 0 | 0 | 0 | 0 | 49k | 1k | 0.05 | 13 |
| shipped-skill | 24/26 | 3 | 2 | 1 | 15k | 0 | 64k | 1k | 0.08 | 11 |
| skill | 14/26 | 4 | 1 | 0 | 3k | 0 | 69k | 1k | 0.06 | 12 |
| mcp | 17/26 | 5 | 2 | 0 | 3k | 0 | 72k | 1k | 0.07 | 11 |
| hook | 22/26 | 2 | 1 | 0 | 4k | 0 | 53k | 1k | 0.06 | 10 |
| instructions | 25/26 | 3 | 2 | 0 | 15k | 0 | 63k | 1k | 0.08 | 11 |

## Rubric items missed

- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: skill#1, mcp#1, skill#2, mcp#2; Acme settlement batch: skill#1, mcp#1, skill#2, mcp#2
- **dashboard** (held out; sibling project (ledger-ui), Codex session): 2 retries: skill#1, shipped-skill#1, instructions#1, hook#1, mcp#1, shipped-skill#2, instructions#2, hook#2, mcp#2; names the ledger-ui project: skill#1, shipped-skill#1, instructions#1, hook#1, mcp#1, skill#2, shipped-skill#2, instructions#2, hook#2, mcp#2
- **signature** (proactive; Codex session, a rule in the user's message after the preview cut): billing-svc imports deliver(): skill#1, mcp#1, skill#2, mcp#2
- **add-dep** (proactive; Codex session, the second rule in the user's message, past the preview cut): no new dependencies without asking: skill#1, mcp#1, skill#2, mcp#2
- **redis-move** (proactive; Claude Code worktree session, the user's second message): on-prem installs have no Redis: skill#1, mcp#1, skill#2
- **timeout** (proactive; Claude Code session, the reason is in the assistant's analysis): raised after duplicate charges: skill#1, shipped-skill#1, instructions#1, hook#1, mcp#1, skill#2, shipped-skill#2, instructions#2, hook#2, mcp#2; names Acme or billing: skill#1, shipped-skill#1, instructions#1, hook#1, mcp#1, skill#2, shipped-skill#2, instructions#2, hook#2, mcp#2

## mmr commands used

- `mmr read session`: 56
- `mmr list sessions`: 42
- `mmr find`: 30
- `mcp__mmr__mmr_find`: 24
- `mmr --source read session`: 12
- `mcp__mmr__mmr_read_session`: 6
- `mmr read project`: 2
- `for id in 64cba498-6ae0-5e58-85ec-5f9c3b`: 2
- `mcp__mmr__mmr_context_project`: 2
- `mcp__mmr__mmr_recall`: 2
- `for s in "pi read session 64cba498-6ae0-`: 1
- `for s in "claude 8a3e4773-4aeb-5975-908b`: 1
- `for s in "codex f9471233-13a2-582c-b8af-`: 1
- `git diff; for s in "claude read session `: 1
- `for p in "codex b88829fd-f7f9-5dc6-8575-`: 1
