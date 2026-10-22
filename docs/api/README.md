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

## MCP server

`mmr mcp --transport stdio` and `mmr mcp --transport http` run the native Rust
MCP server. Stdio is the simplest setup for a local MCP client: it calls the
same Rust command handlers directly, without starting or probing a REST server.
It does not reserve a TCP port. The former `MMR_MCP_REST_BACKING_BIND` and
`MMR_MCP_REST_BACKING_BASE_URL` settings are ignored. Use explicit HTTP transport
when a client needs a listener; its bind address and mount path remain configurable.

```sh
mmr mcp --transport stdio
mmr mcp --transport http --bind 127.0.0.1:8765 --path /mcp
```

## Runnable synthetic stdio example

From the repository root, with Python 3 and a Rust build toolchain:

```bash
cargo build --locked
python3 examples/standalone_stdio.py
```

The [example](../../examples/standalone_stdio.py) creates a temporary HOME and
Codex session, starts the real MCP subprocess with a minimal environment and
unusable REST settings, then initializes, lists tools and reads the session.
It checks JSON-only protocol output, unchanged native files and clean EOF
shutdown. It opens no network sockets and reads no existing user history.

Expected output:

```text
Stdio initialized with unusable REST settings; no REST server needed.
Listed 19 MCP tools.
user: Keep the retry limit at three.
assistant: Noted for the next session.
Protocol stdout was JSON only; native histories unchanged; clean EOF shutdown.
Temporary home removed.
```

For an already built binary, append its path:
`python3 examples/standalone_stdio.py ./target/release/mmr`.
