# Cross-harness context eval: proactive-claude

- mmr: `mmr 0.2.0` (7b4b671)
- agent: Claude Code 2.1.323 (Claude Code), model `claude-sonnet-5-5`
- runs: 40 (2 per cell), 2026-11-13T00:20

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | signature | add-dep | redis-move | timeout | all |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| none | – | – | – | – | – | – | 0/2 | – | – | 0/2 | 0/2 | 0/2 | 0/2 | **0/10** |
| skill | – | – | – | – | – | – | 0/2 | – | – | 0/2 | 0/2 | 1/2 | 0/2 | **1/10** |
| hook | – | – | – | – | – | – | 2/2 | – | – | 2/2 | 2/2 | 2/2 | 0/2 | **8/10** |
| instructions | – | – | – | – | – | – | 2/2 | – | – | 2/2 | 2/2 | 2/2 | 2/2 | **10/10** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/10 | 4 | 0 | 0 | 0 | 0 | 90k | 2k | 0.07 | 20 |
| skill | 1/10 | 4 | 0 | 0 | 0 | 0 | 91k | 2k | 0.07 | 17 |
| hook | 8/10 | 2 | 1 | 2 | 5k | 0 | 54k | 1k | 0.06 | 12 |
| instructions | 10/10 | 4 | 2 | 0 | 16k | 0 | 82k | 1k | 0.08 | 12 |

## Rubric items missed

- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: skill#1, skill#2; Acme settlement batch: skill#1, skill#2
- **signature** (proactive; Codex session, a rule in the user's message after the preview cut): billing-svc imports deliver(): skill#1, skill#2
- **add-dep** (proactive; Codex session, the second rule in the user's message, past the preview cut): no new dependencies without asking: skill#1, skill#2
- **redis-move** (proactive; Claude Code worktree session, the user's second message): on-prem installs have no Redis: skill#2
- **timeout** (proactive; Claude Code session, the reason is in the assistant's analysis): names Acme or billing: skill#1, hook#1, skill#2, hook#2; raised after duplicate charges: hook#1, skill#2, hook#2

## mmr commands used

- `mmr read session`: 19
- `mmr list sessions`: 10
- `mmr find`: 1
