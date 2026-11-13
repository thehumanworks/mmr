# Cross-harness context eval: codex-mcp-readonly

- mmr: `mmr 0.2.0` (a03c863)
- agent: codex-cli 0.167.0-alpha.4.1, model `gpt-6.1-sol`
- runs: 32 (2 per cell), 2026-11-13T03:29

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | signature | add-dep | redis-move | timeout | replay-delete | log-payload | backoff | all |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| mcp | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 0/2 | 2/2 | 0/2 | 2/2 | 0/2 | 2/2 | 0/2 | 0/2 | **20/32** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| mcp | 26/32 | 6 | 3 | 0 | 10k | 0 | 86k | 534 | 0.00 | 26 |

## Rubric items missed

- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: mcp#1, mcp#2; Acme settlement batch: mcp#1, mcp#2
- **dashboard** (held out; sibling project (ledger-ui), Codex session): names the ledger-ui project: mcp#1, mcp#2
- **add-dep** (proactive; Codex session, the second rule in the user's message, past the preview cut): no new dependencies without asking: mcp#1, mcp#2
- **timeout** (proactive; Claude Code session, the reason is in the assistant's analysis): raised after duplicate charges: mcp#1, mcp#2; names Acme or billing: mcp#1, mcp#2
- **log-payload** (proactive; Grok session reviewing logs, the user's second message): payloads hold customers' IBANs: mcp#1, mcp#2
- **backoff** (proactive; Pi session about request rates, the user's second message): Acme's rate limit (5 req/s, or an hour's block): mcp#1, mcp#2

## mmr commands used

- `mcp__mmr__mmr_read_session`: 56
- `mcp__mmr__mmr_find`: 36
- `mcp__mmr__mmr_context_project`: 4
- `mcp__mmr__mmr_recall`: 2
