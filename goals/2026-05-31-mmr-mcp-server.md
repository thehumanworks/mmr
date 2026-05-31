---
title: "Expose mmr as an MCP server"
description: "Add `mmr mcp --transport http|stdio` using the official Rust MCP SDK, with read/query tools and prompt templates that preserve mmr CLI contracts."
date: 2026-05-31
status: done
---

# GOAL: Expose `mmr` as an MCP server

## Outcome

`mmr` should run as a Model Context Protocol server from the installed CLI:

```bash
mmr mcp --transport stdio
mmr mcp --transport http
```

The server must expose mmr history/query functionality as MCP tools and reusable agent workflows as MCP prompts. It should use the official Rust MCP SDK, `modelcontextprotocol/rust-sdk` (`rmcp`), and preserve the CLI's current project, source, JSON, and verification contracts.

This goal document was the implementation prompt. Implementation is complete because the status is `done`.

## Completion Report

Implemented on 2026-05-31:

- Added `mmr mcp --transport stdio` with protocol-clean stdout.
- Added `mmr mcp --transport http` using rmcp streamable HTTP on loopback with MCP mounted at `/mcp`.
- Added all required V1 MCP tools from this goal.
- Added all required V1 MCP prompts from this goal.
- Added fixture-backed protocol tests in `tests/mcp_contract.rs` covering initialization capabilities, tool listing, prompt listing, string numeric prompt args, stdio subprocess framing, HTTP initialization, required-source error behavior, and representative CLI-equivalence calls.
- Updated top-level CLI help with MCP examples and the stdio stdout framing warning.
- Kept `.agents/skills/mmr-mcp-rust-sdk` valid with the project-local rmcp gotchas reference.

Verification passed:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
python3 <release-binary stdio/http JSON-RPC smoke harness>
python3 /Users/mish/.codex/skills/.system/skill-creator/scripts/quick_validate.py .agents/skills/mmr-mcp-rust-sdk
```

## Definition Of Done

- `mmr mcp --transport stdio` starts a protocol-clean stdio MCP server.
- `mmr mcp --transport http` starts a streamable HTTP MCP server on loopback, serving MCP at `/mcp`.
- Server initialization advertises both `tools` and `prompts` capabilities.
- Representative MCP tool calls return the same JSON semantics as the corresponding CLI commands against fixture data.
- Representative MCP prompts list and resolve with arguments.
- Stdio mode never writes human diagnostics to stdout after transport startup.
- The verification loop passes: `cargo fmt`, `cargo test`, ignored benchmark, strict clippy, and release build.
- Project-local skill `.agents/skills/mmr-mcp-rust-sdk` remains valid and documents rmcp gotchas for future work.

## Current Research

Official SDK repo: `https://github.com/modelcontextprotocol/rust-sdk`.

Important findings from `rmcp`:

- `rmcp::transport::stdio()` returns tokio stdin/stdout transport handles for stdio servers.
- Streamable HTTP server support is exposed through `rmcp::transport::streamable_http_server::{StreamableHttpService, session::local::LocalSessionManager}` and is usually mounted with axum at `/mcp`.
- Tools use `#[tool_router]`, `#[tool]`, and `#[tool_handler]`.
- Prompts use `#[prompt_router]`, `#[prompt]`, and `#[prompt_handler]`.
- For a server with both tools and prompts, use an explicit `impl ServerHandler` with stacked `#[tool_handler]` and `#[prompt_handler]`; avoid `#[tool_router(server_handler)]`, which is tools-only oriented.
- Prompt arguments may arrive as strings even when semantically numeric or boolean. Parse both string and native JSON forms.
- HTTP session data is transport-specific; core mmr semantics must not depend on HTTP-only headers or session IDs.

See `.agents/skills/mmr-mcp-rust-sdk/references/rmcp-server-patterns.md` before implementation.

## Architecture

### CLI Surface

Add a new top-level command:

```text
mmr mcp --transport stdio
mmr mcp --transport http [--bind 127.0.0.1:8765] [--path /mcp]
```

Decisions:

- `--transport` is required and accepts `stdio` or `http`.
- HTTP defaults to `127.0.0.1:8765` and path `/mcp`.
- Non-loopback bind is a follow-up unless explicitly requested. If allowed later, warn on stderr and document the trust boundary.
- `mmr mcp --transport stdio` must not print normal JSON command output through `run_cli`; it should enter the MCP server loop directly.

### Module Layout

Preferred file ownership:

- `src/mcp.rs` or `src/mcp/mod.rs`: MCP server struct, arg types, tool handlers, prompt handlers, transport runners.
- `src/cli.rs`: clap enum and routing into `mcp::run_mcp(args).await`.
- `src/main.rs`: remain thin.
- `Cargo.toml`: add `rmcp`, `schemars` if needed directly, and `axum` for HTTP serving.
- `tests/mcp_contract.rs`: protocol-level tests.
- `.agents/skills/mmr-mcp-rust-sdk/`: project-local reusable skill for rmcp gotchas.

Keep existing CLI response structs in `src/types/` as the primary response contract. If MCP needs wrapper metadata, add it narrowly rather than inventing parallel response types.

### Server Struct

Use a cloneable server with routers:

```rust
#[derive(Clone)]
pub struct MmrMcpServer {
    tool_router: ToolRouter<MmrMcpServer>,
    prompt_router: PromptRouter<MmrMcpServer>,
}
```

Do not store a loaded `QueryService` globally unless tests prove it is safe. Loading per read call is simpler and keeps behavior close to current CLI invocation semantics. If performance later matters, add an internal cache with explicit invalidation.

### Transport Runners

Stdio runner:

- Build `MmrMcpServer::new()`.
- Call `.serve(rmcp::transport::stdio()).await`.
- Wait on `service.waiting().await`.
- Log only to stderr or tracing sinks that do not touch stdout.

HTTP runner:

- Build `StreamableHttpService::new(|| Ok(MmrMcpServer::new()), LocalSessionManager::default().into(), Default::default())`.
- Mount under `/mcp` with `axum::Router::new().nest_service(path, service)`.
- Bind loopback.
- Support graceful shutdown with Ctrl-C for manual use.
- Print startup URL to stderr, not stdout, if anything is printed.

## MCP Tools

Expose read/query tools first. These are safe for agent use and align with mmr's core purpose.

### Required V1 Tools

| Tool | CLI Equivalent | Notes |
|---|---|---|
| `mmr_list_projects` | `mmr list projects` | Args: `source`, `limit`, `offset`, `sort_by`, `order`. |
| `mmr_list_sessions` | `mmr list sessions` | Args: `source`, `project`, `all`, `limit`, `offset`, `sort_by`, `order`. |
| `mmr_read_session` | `mmr read session <id>` | Args: `session_id`, `source`, `project`, `limit`, `offset`. |
| `mmr_read_project` | `mmr read project` | Args: `source`, `project`, `limit`, `offset`. |
| `mmr_read_source` | `mmr --source <source> read source` | Require explicit source. |
| `mmr_recall` | `mmr recall [N]` | Args: `n`, `source`, `project`, `all`, `limit`, `include_newest`. |
| `mmr_find` | `mmr find <query>` | Args: `query`, `project`, `session`, `role`, `ignore_case`, `context`, `format`. |
| `mmr_context_project` | `mmr context project` | Args: `source`, `project`, `limit`. |
| `mmr_context_source` | `mmr --source <source> context source` | Require explicit source. |
| `mmr_assimilate_project` | `mmr assimilate project` | Return prompt/runbook/output contract/evidence. |
| `mmr_assimilate_source` | `mmr --source <source> assimilate source` | Require explicit source. |
| `mmr_summarize_project` | `mmr summarize project` | Requires `OPENAI_API_KEY`; surface provider errors as MCP errors. |
| `mmr_summarize_session` | `mmr summarize session <id>` | Args: `session_id`, `source`, `project`, `instructions`, `model`, `output_format`. |
| `mmr_summarize_source` | `mmr --source <source> summarize source` | Require explicit source. |
| `mmr_status` | `mmr status` | Read-only diagnostics. |
| `mmr_skill_load` | `mmr skill load` | Return bundled skill text. |

### Deferred Tools

Do not expose by default in V1:

- `mmr import`: writes local store.
- `mmr note`: writes human-authored memory.
- `mmr redact scan`: writes redaction runs.
- `mmr sync`: can contact remote and reconcile state.
- `mmr teleport receive/apply/resume/export/send/serve`: side effects, network, or native provider writes.

If a later implementation exposes these, require explicit tool names with side-effect annotations and targeted tests.

### Tool Response Format

Return one text content item containing JSON for structured responses:

```json
{
  "command": "list/projects",
  "projects": []
}
```

Rationale: current mmr surfaces are JSON-first, and MCP clients can parse text JSON consistently. Avoid double-encoding when returning raw markdown from summarize or skill load; wrap with an explicit response object if needed.

## MCP Prompts

Prompts should encode reusable agent workflows, not replace tools. They should produce prompt messages that tell the client model which mmr tool to call and how to use the result.

### Required V1 Prompts

| Prompt | Args | Purpose |
|---|---|---|
| `mmr_recall_previous_session` | `project`, `source`, `n`, `limit` | Ask the agent to retrieve and summarize the previous stable session for continuity. |
| `mmr_project_context_brief` | `project`, `source`, `limit` | Ask the agent to build a compact project context brief from sessions and messages. |
| `mmr_session_handoff` | `session_id`, `source`, `project` | Ask the agent to read one session and produce a continuation handoff. |
| `mmr_memory_assimilation` | `project`, `source`, `evidence_mode` | Ask the agent to run the assimilate tool and produce memory candidates with evidence. |
| `mmr_find_then_read` | `query`, `project`, `source` | Ask the agent to search history, inspect matching sessions, and cite the exact session IDs used. |

Prompt handlers should accept optional string args and parse integer options from either strings or JSON numbers.

## Implementation Plan

1. Add the project-local skill and keep it validated.
2. Add dependencies and `McpArgs`/`McpTransportArg` to clap without changing existing commands.
3. Add `src/mcp.rs` with server struct, transport runners, and one minimal tool plus one prompt.
4. Add protocol-level tests that fail until MCP init/list tools/list prompts work.
5. Fill in required V1 tools by delegating to existing service/helper functions, not subprocesses.
6. Fill in required V1 prompts and argument parsing.
7. Add stdio subprocess smoke that proves stdout is protocol-only.
8. Add HTTP smoke on `127.0.0.1:0` or an equivalent test helper if the CLI bind parser needs a concrete port.
9. Update CLI help examples and specs only after behavior is covered.
10. Run the full verification loop and update this file's status to `done` only after green checks.

## Acceptance Tests

Add fixture-driven tests under `tests/mcp_contract.rs`:

- `mcp_server_initializes_with_tools_and_prompts`: MCP initialize result advertises both capabilities.
- `mcp_lists_expected_tools`: list tools includes every required V1 tool.
- `mcp_lists_expected_prompts`: list prompts includes every required V1 prompt.
- `mcp_list_projects_matches_cli_fixture`: `mmr_list_projects` output matches seeded fixture totals and source semantics.
- `mcp_read_session_matches_cli_fixture`: `mmr_read_session` returns chronological messages for `sess-claude-1`.
- `mcp_read_source_requires_explicit_source`: missing source returns an MCP invalid params error.
- `mcp_prompt_accepts_string_numeric_args`: prompt `n` or `limit` works when passed as string.
- `mcp_stdio_subprocess_protocol_smoke`: spawned `mmr mcp --transport stdio` initializes and does not emit non-protocol stdout.
- `mcp_http_streamable_smoke`: HTTP transport initializes at `/mcp`.

Where direct protocol client tests are simpler than subprocess tests, use in-memory duplex transport for handler coverage and keep one subprocess smoke per transport.

## Validation

Run the exact repo loop before closing:

```bash
cargo fmt
cargo test
cargo test --test cli_benchmark -- --ignored --nocapture
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Also run manual smoke commands and record the result in the final implementation report:

```bash
mmr mcp --transport stdio
mmr mcp --transport http
```

For stdio, use an MCP client or scripted JSON-RPC harness rather than visual inspection. For HTTP, use a client that speaks streamable HTTP MCP against `/mcp`; a plain browser GET is not enough.

## Risks And Guardrails

- Stdio stdout pollution can silently break clients. Keep all startup text on stderr.
- MCP prompt args may be strings. Parse defensively.
- Tool and prompt capability generation can be lost if handler macros are wired incorrectly. Assert capabilities in tests.
- HTTP session IDs are transport details. Do not use them for mmr session selection.
- Do not broaden to write/sync/teleport tools without a new explicit safety design.
- Keep default CLI JSON bytes and behavior unchanged for non-MCP commands.

## Non-Goals

- No implementation in the goal-authoring turn.
- No old SSE transport unless `rmcp` requires it for compatibility.
- No remote/provider proof beyond local MCP protocol proof for V1.
- No mutation tools in V1.
