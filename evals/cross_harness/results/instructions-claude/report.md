# Cross-harness context eval: instructions-claude

- mmr: `mmr 0.2.0` (7b4b671)
- agent: Claude Code 2.1.323 (Claude Code), model `claude-sonnet-5-5`
- runs: 16 (2 per cell), 2026-11-13T00:22

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | signature | add-dep | redis-move | timeout | all |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| instructions | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | – | 2/2 | 0/2 | – | – | – | – | **14/16** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| instructions | 16/16 | 3 | 2 | 0 | 15k | 0 | 62k | 1k | 0.08 | 10 |

## Rubric items missed

- **dashboard** (held out; sibling project (ledger-ui), Codex session): 2 retries: instructions#1, instructions#2; names the ledger-ui project: instructions#1, instructions#2

## mmr commands used

- `mmr list sessions`: 13
- `mmr --source read session`: 9
- `mmr find`: 4
- `mmr read session`: 4
- `for id in 64cba498-6ae0-5e58-85ec-5f9c3b`: 2
- `for s in "claude 8a3e4773-4aeb-5975-908b`: 1
