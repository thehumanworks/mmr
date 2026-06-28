# mmr Retrieval Docs Overview

view: Human

This local docs site is the source of truth for the search-to-read retrieval
feature. It describes the finished `mmr retrieve <query>` workflow before code
is written, so implementation can work backwards from the product contract.

The feature turns a remembered clue, such as an error string or file path, into
a bounded packet of prior-session context. It keeps `mmr find` as exact literal
search, adds `mmr retrieve` for the higher-level "search then read" workflow,
and keeps output small enough for agents to consume without pulling entire
transcripts.

Open the retrieval pipeline:

- Human route: `/human/retrieve/`
- Agent route: `/agent/retrieve/`

Run locally from the repository root:

```bash
python3 -m http.server 8000
open http://127.0.0.1:8000/docs/site/index.html
curl -s http://127.0.0.1:8000/.well-known/agents.json | jq .
```
