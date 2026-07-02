# MMR REST API

Generated API documentation for MMR's REST contract.

- OpenAPI source: [openapi.json](./openapi.json)
- Static docs: [index.html](./index.html)
- OpenAPI version: 3.1.0
- Operations: 24
- Schemas: 102

Regenerate after contract edits:

```sh
node scripts/generate-api-docs.mjs
```

## Python FastMCP bootstrap

`mmr mcp --transport stdio` and `mmr mcp --transport http` run the native Rust
MCP server. Use `mmr mcp python-bootstrap` only when you want a Python FastMCP
bridge generated from the REST OpenAPI contract.

```sh
mmr mcp python-bootstrap > server.py
mmr mcp python-bootstrap --write server.py
mmr mcp python-bootstrap --run --transport stdio
mmr mcp python-bootstrap --run --transport http --host 127.0.0.1 --port 8766
mmr mcp python-bootstrap --dry-run
mmr mcp python-bootstrap --run --no-auto-rest --api-base-url http://127.0.0.1:8765
```

By default, `--run` auto-starts an internal loopback mmr REST backing server on
an ephemeral port, serves the OpenAPI document from it, and passes that URL to
Python. Use `--no-auto-rest` with `--api-base-url` or `--openapi-url` only when
you already have an external REST backing server.

The launcher passes these environment variables to the Python process:

- `API_BASE_URL` — REST API base URL; default run mode uses the auto-started REST backing URL.
- `OPENAPI_URL` — OpenAPI URL or local JSON path; default run mode uses the auto-started backing server's `/openapi.json`.
- `API_TOKEN_ENV` — name of the optional bearer-token env var, default `API_TOKEN`.
- `MCP_TRANSPORT` — `stdio` or `http`.
- `MCP_HOST` / `MCP_PORT` — HTTP bind settings for Python FastMCP.

`--print`, `--write`, and `--dry-run` do not import FastMCP. `--run` starts
Python and requires:

```sh
python3 -m pip install fastmcp httpx
```
