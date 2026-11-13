# Cross-harness context eval: merged-haiku

- mmr: `mmr 0.2.0` (10383c3)
- agent: Claude Code 2.1.323 (Claude Code), model `claude-haiku-5-5`
- runs: 36 (1 per cell), 2026-11-12T23:57

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | all |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/1 | 0/1 | 0/1 | 0/1 | 0/1 | 0/1 | 0/1 | 0/1 | 0/1 | **0/9** |
| skill | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 0/1 | 1/1 | 0/1 | **7/9** |
| mcp | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 0/1 | 0/1 | 0/1 | **6/9** |
| hook | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 1/1 | 0/1 | **8/9** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/9 | 3 | 0 | 0 | 0 | 0 | 67k | 1k | 0.00 | 9 |
| skill | 7/9 | 3 | 1 | 0 | 11k | 0 | 69k | 2k | 0.00 | 10 |
| mcp | 8/9 | 4 | 2 | 0 | 6k | 0 | 74k | 2k | 0.00 | 10 |
| hook | 8/9 | 3 | 1 | 0 | 6k | 0 | 63k | 1k | 0.00 | 8 |

## Rubric items missed

- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: skill#1, mcp#1; Acme settlement batch: mcp#1
- **flaky** (held out; Cursor session): test_store: mcp#1; cause: CI jobs share one DB file (database is locked): mcp#1
- **dashboard** (held out; sibling project (ledger-ui), Codex session): 2 retries: skill#1, hook#1; names the ledger-ui project: skill#1, hook#1, mcp#1

## mmr commands used

- `mcp__mmr__mmr_find`: 13
- `mmr find`: 10
- `mmr read session`: 7
- `mmr --source read session`: 4
- `mcp__mmr__mmr_read_session`: 3
- `mcp__mmr__mmr_context_project`: 2
- `mmr recall`: 1
- `for s in 8a3e4773-4aeb-5975-908b-fb411af`: 1
- `mcp__mmr__mmr_recall`: 1
