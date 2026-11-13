# Cross-harness context eval: codex

- mmr: `mmr 0.2.0` (f80b5d7)
- agent: codex-cli 0.167.0-alpha.4.1, model `gpt-6.1-sol`
- runs: 128 (2 per cell), 2026-11-13T03:06

## Pass rate (all rubric items found in the final answer)

| condition | why-retries | resume | constraints | redis | staging-url | retry-history | deploy | flaky | dashboard | signature | add-dep | redis-move | timeout | replay-delete | log-payload | backoff | all |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/2 | 0/2 | 1/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **1/32** |
| shipped-skill | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 0/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | **30/32** |
| skill | 2/2 | 2/2 | 1/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | 1/2 | 2/2 | 0/2 | 2/2 | 2/2 | 2/2 | 2/2 | 2/2 | **28/32** |
| mcp | 0/2 | 1/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | 0/2 | **1/32** |

## Cost per run (median over all tasks; mmr errors and raw history reads are totals)

| condition | runs using mmr | tool calls | mmr calls | mmr errors (total) | mmr output chars | raw history reads (total) | input tokens | output tokens | cost USD | wall s |
|---|---|---|---|---|---|---|---|---|---|---|
| none | 0/32 | 6 | 0 | 0 | 0 | 8 | 94k | 551 | 0.00 | 28 |
| shipped-skill | 32/32 | 7 | 3 | 1 | 12k | 0 | 89k | 643 | 0.00 | 27 |
| skill | 32/32 | 6 | 2 | 1 | 15k | 0 | 79k | 651 | 0.00 | 30 |
| mcp | 26/32 | 4 | 1 | 28 | 0 | 0 | 68k | 527 | 0.00 | 27 |

## Rubric items missed

- **why-retries** (Claude Code session, main project path): duplicate charges: mcp#1, mcp#2; names Acme or billing: mcp#1, mcp#2
- **resume** (Pi session (newest before this one)): migration 0007 next: mcp#2; 72-hour key cleanup: mcp#2
- **constraints** (Codex session (current Codex format), user message): keep deliver() signature: mcp#1, skill#2, mcp#2; because billing-svc imports it: mcp#1, mcp#2; no new dependencies without asking: mcp#1, mcp#2
- **redis** (Claude Code session run in a worktree under .claude/worktrees): mentions Redis was tried: mcp#1, mcp#2; reason: on-prem/ops has no Redis: mcp#1, mcp#2
- **staging-url** (Grok session run from the scripts/ subdirectory; a detail mentioned once): exact staging URL: mcp#1, mcp#2
- **retry-history** (synthesis across an archived Codex session, a Claude Code session and a Cursor session): cut after Acme/billing duplicate charges: mcp#1, mcp#2; jitter after Acme's outage: mcp#1, mcp#2
- **deploy** (held out; Codex session (current format), aside in a user message of an unrelated task): 17:00-19:00 CET window: mcp#1, mcp#2; Acme settlement batch: mcp#1, mcp#2
- **flaky** (held out; Cursor session): test_store: mcp#1, mcp#2; cause: CI jobs share one DB file (database is locked): mcp#1, mcp#2
- **dashboard** (held out; sibling project (ledger-ui), Codex session): 2 retries: skill#1, mcp#1, mcp#2; names the ledger-ui project: skill#1, mcp#1
- **signature** (proactive; Codex session, a rule in the user's message after the preview cut): billing-svc imports deliver(): mcp#1, mcp#2
- **add-dep** (proactive; Codex session, the second rule in the user's message, past the preview cut): no new dependencies without asking: skill#1, shipped-skill#1, mcp#1, skill#2, shipped-skill#2, mcp#2
- **redis-move** (proactive; Claude Code worktree session, the user's second message): on-prem installs have no Redis: mcp#1, mcp#2
- **timeout** (proactive; Claude Code session, the reason is in the assistant's analysis): raised after duplicate charges: mcp#1, mcp#2; names Acme or billing: mcp#1, mcp#2
- **replay-delete** (proactive; Cursor session about dead letters, the user's second message): on-call runbook uses it: mcp#1, mcp#2
- **log-payload** (proactive; Grok session reviewing logs, the user's second message): payloads hold customers' IBANs: mcp#1, mcp#2
- **backoff** (proactive; Pi session about request rates, the user's second message): Acme's rate limit (5 req/s, or an hour's block): mcp#1, mcp#2

## mmr commands used

- `mmr read session`: 93
- `mmr find`: 72
- `mcp__mmr__mmr_find`: 24
- `mmr read project`: 13
- `mmr list sessions`: 10
- `mmr recall`: 4
- `mcp__mmr__mmr_recall`: 2
- `mcp__mmr__mmr_context_project`: 2
- `mmr --source read session`: 1
