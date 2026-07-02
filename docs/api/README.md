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
MCP server. The stdio command is the default setup path for MCP clients: it
checks the local loopback REST backing endpoint and starts one when none is
already available, so users do not need to start a separate REST API process.

```sh
mmr mcp --transport stdio
mmr mcp --transport http --bind 127.0.0.1:8765 --path /mcp
```
