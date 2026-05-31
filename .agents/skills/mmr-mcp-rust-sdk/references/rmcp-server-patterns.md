# rmcp Server Patterns For mmr

Source: official Rust SDK repo `modelcontextprotocol/rust-sdk`, crate `rmcp`.

## Cargo Features

`rmcp` default features include `base64`, `macros`, and `server`, but mmr should name the transport features explicitly:

```toml
rmcp = { version = "...", features = [
  "server",
  "macros",
  "transport-io",
  "transport-streamable-http-server",
  "schemars"
] }
axum = { version = "0.8", default-features = false, features = ["http1", "tokio"] }
```

Use the released crate if available in the lockfile flow. If the implementation must pin git temporarily, document why in the goal and replace it before shipping if crates.io has the needed release.

## Server Shape

The SDK examples use a cloneable server struct that owns routers:

```rust
#[derive(Clone)]
pub struct MmrMcpServer {
    tool_router: ToolRouter<MmrMcpServer>,
    prompt_router: PromptRouter<MmrMcpServer>,
}

#[tool_router]
impl MmrMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }
}

#[prompt_router]
impl MmrMcpServer {
    // prompt handlers
}

#[tool_handler(name = "mmr", version = env!("CARGO_PKG_VERSION"))]
#[prompt_handler]
impl ServerHandler for MmrMcpServer {}
```

If tools and prompts are on the same server, keep the explicit stacked handler impl. Do not rely on `#[tool_router(server_handler)]`, which is intended for tools-only servers.

## Tool Handlers

Use `#[tool]` on functions inside a `#[tool_router]` impl. Use `Parameters<T>` with `serde::Deserialize` and `schemars::JsonSchema` args. Return `Result<CallToolResult, ErrorData>` and put JSON in text content unless a richer MCP content type is deliberately chosen.

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReadSessionArgs {
    pub session_id: String,
    pub source: Option<String>,
    pub project: Option<String>,
    pub limit: Option<usize>,
}

#[tool(description = "Read one mmr session by ID")]
async fn read_session(
    &self,
    Parameters(args): Parameters<ReadSessionArgs>,
) -> Result<CallToolResult, ErrorData> {
    let value = self.read_session_json(args).map_err(to_mcp_error)?;
    Ok(CallToolResult::success(vec![Content::text(value.to_string())]))
}
```

## Prompt Handlers

Use `#[prompt]` inside a `#[prompt_router]` impl. Prompt handlers can return either `Vec<PromptMessage>` or `GetPromptResult`.

MCP prompt arguments are specified as a string map. Some clients send all values as strings, even for numbers and booleans. For numeric args, accept both string and native JSON forms using a custom deserializer.

```rust
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProjectContextPromptArgs {
    pub project: Option<String>,
    pub source: Option<String>,
    #[serde(default, deserialize_with = "string_or_usize_opt")]
    pub limit: Option<usize>,
}
```

## Transports

Stdio:

```rust
let service = MmrMcpServer::new().serve(rmcp::transport::stdio()).await?;
service.waiting().await?;
```

Streamable HTTP:

```rust
let service = StreamableHttpService::new(
    || Ok(MmrMcpServer::new()),
    LocalSessionManager::default().into(),
    Default::default(),
);
let router = axum::Router::new().nest_service("/mcp", service);
let listener = tokio::net::TcpListener::bind(bind_addr).await?;
axum::serve(listener, router).await?;
```

HTTP-specific request/session data is available from `RequestContext<RoleServer>::extensions`; examples read the `mcp-session-id` header from axum request parts. Do not make core mmr behavior depend on HTTP-only session state.

## Testing Notes

Use fixture-backed tests from `tests/common/mod.rs`. Good coverage should include:

- Handler-level MCP client/server tests using in-memory duplex transport.
- CLI subprocess stdio smoke for `mmr mcp --transport stdio`, ensuring initialization/list tools/list prompts works.
- HTTP smoke that binds `127.0.0.1:0`, calls `/mcp`, and verifies streamable HTTP initialization.
- Equivalence tests comparing MCP tool output to existing CLI JSON for seeded fixtures.

Keep protocol tests deterministic and avoid real local history unless a manual smoke is explicitly marked outside the automated suite.
