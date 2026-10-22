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
