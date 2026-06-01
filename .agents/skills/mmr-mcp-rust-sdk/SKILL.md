---
name: mmr-mcp-rust-sdk
description: Build or review mmr's Rust Model Context Protocol server using the official `modelcontextprotocol/rust-sdk` (`rmcp`). Use when adding `mmr mcp`, stdio or streamable HTTP transports, MCP tools, MCP prompts, rmcp macros, or MCP contract tests in this repo.
---

# mmr MCP Rust SDK

Use this skill when implementing or reviewing `mmr mcp` in this Rust CLI. It keeps the non-obvious `rmcp` SDK details close to the repo so future agents do not rediscover them.

## Workflow

1. Re-read `AGENTS.md`, `.cursor/rules/verification-loop.mdc`, and `.cursor/rules/cli-contract.mdc`.
2. Keep MCP server code in a dedicated module, for example `src/mcp.rs` or `src/mcp/mod.rs`, and keep `src/main.rs` as the thin clap entrypoint.
3. Prefer direct calls into existing `QueryService`, `ai`, `dream`, `status`, and formatting helpers over spawning the `mmr` binary.
4. For stdio, never write human diagnostics to stdout after transport startup. stdout belongs to MCP frames only.
5. For HTTP, use streamable HTTP, nest the service at `/mcp`, and default bind to loopback.
6. Test the server through MCP protocol calls, not only by asserting Rust helper functions.

## rmcp Patterns

Load [references/rmcp-server-patterns.md](references/rmcp-server-patterns.md) before changing MCP server code. The reference covers:

- Cargo features for stdio and streamable HTTP server support.
- `#[tool_router]`, `#[tool_handler]`, `#[prompt_router]`, and `#[prompt_handler]`.
- Combining tools and prompts without losing capabilities in `get_info()`.
- MCP prompt argument string parsing gotchas.
- Local session behavior for streamable HTTP.

## mmr Contract

Expose mmr data with the same semantics as the CLI:

- `--source` means one of `claude`, `codex`, `cursor`, `grok`, or `pi`; omitted means all sources unless `MMR_DEFAULT_SOURCE` applies.
- Project scoping should match current CLI project detection and alias resolution.
- Tool responses should preserve existing JSON response structs where practical.
- Read/query tools should include source and project metadata on returned items.
- Side-effecting commands need explicit design before exposure; do not quietly turn `sync`, `import bundle`, `share session`, `redact`, or native bundle apply flows into MCP tools.

## Verification

At minimum, add a focused MCP contract test file and run the repo verification loop:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Add protocol-level checks for:

- `mmr mcp --transport stdio` initializes and exposes tools plus prompts.
- HTTP streamable service responds at `/mcp`.
- Representative tools return the same JSON as CLI-backed fixture expectations.
- Representative prompts list and resolve with arguments.
- stderr/stdout separation is preserved for stdio.
