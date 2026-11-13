use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Result, bail};
use axum::Router;
use clap::{Args, Parser, ValueEnum};
use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, GetPromptRequestParams, GetPromptResult, Implementation,
    ListPromptsResult, PaginatedRequestParams, PromptMessage, PromptMessageRole,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt, prompt, prompt_handler, prompt_router, tool,
    tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::cli::{Cli, run_cli};

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Transport to serve MCP over: stdio or streamable HTTP.
    ///
    /// Stdio runs directly without a REST server or listener.
    #[arg(long, value_enum)]
    pub transport: Option<McpTransportArg>,
    /// HTTP bind address. Ignored for stdio.
    #[arg(long, default_value = "127.0.0.1:8765")]
    pub bind: SocketAddr,
    /// HTTP mount path. Ignored for stdio.
    #[arg(long, default_value = "/mcp")]
    pub path: String,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "kebab-case")]
pub enum McpTransportArg {
    Stdio,
    Http,
}

pub async fn run_mcp(args: &McpArgs, _pretty: bool) -> Result<String> {
    match args.transport {
        Some(McpTransportArg::Stdio) => run_stdio().await?,
        Some(McpTransportArg::Http) => run_http(args.bind, &args.path).await?,
        None => bail!(
            "missing --transport <stdio|http>; use `mmr mcp --transport stdio`, \
             or `mmr mcp --transport http`"
        ),
    }
    Ok(String::new())
}

async fn run_stdio() -> Result<()> {
    let service = MmrMcpServer::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

async fn run_http(bind: SocketAddr, path: &str) -> Result<()> {
    let router = streamable_http_router(
        path,
        StreamableHttpServerConfig::default()
            .with_stateful_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None),
    );
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let local_addr = listener.local_addr()?;
    eprintln!("mmr MCP server listening on http://{local_addr}{path}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

pub fn streamable_http_router(path: &str, config: StreamableHttpServerConfig) -> Router {
    let service: StreamableHttpService<MmrMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(MmrMcpServer::new()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
    Router::new().nest_service(path, service)
}

#[derive(Debug, Clone)]
pub struct MmrMcpServer {
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

#[tool_router]
impl MmrMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    /// List known projects with source coverage and recency metadata. Use to check whether
    /// prior session history exists for a codebase before deeper retrieval, or to map which
    /// projects have history across Claude, Codex, Cursor, Grok, and Pi.
    #[tool(
        name = "mmr_list_projects",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_projects(
        &self,
        Parameters(args): Parameters<ListProjectsToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// List a project's sessions, newest first, across every harness. Use to see what past
    /// sessions exist before reading one; pass a session's session_id and source to
    /// mmr_read_session.
    #[tool(
        name = "mmr_list_sessions",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn list_sessions(
        &self,
        Parameters(args): Parameters<ListSessionsToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Read one session's messages by session_id. Use after mmr_find, mmr_list_sessions or
    /// mmr_recall points at a session; pass its source too when you have it.
    #[tool(
        name = "mmr_read_session",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn read_session(
        &self,
        Parameters(args): Parameters<ReadSessionToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Read a project's raw message history across every harness, in time order. Use to
    /// reconstruct how work evolved when raw detail matters more than a summary.
    #[tool(
        name = "mmr_read_project",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn read_project(
        &self,
        Parameters(args): Parameters<ReadProjectToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Retrieve the previous stable session for immediate continuity. Reach for this FIRST
    /// when resuming or continuing earlier work, when the user says "continue", "as we
    /// discussed", or "last time", or after compaction, /clear, or a fresh session on an
    /// ongoing task.
    #[tool(
        name = "mmr_recall",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn recall(
        &self,
        Parameters(args): Parameters<RecallToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Find text in this project's earlier agent sessions from every harness, newest first
    /// (case-insensitive; no setup). Use before explaining why code is the way it is, before
    /// proposing an approach that may already have been tried or rejected, before risky steps
    /// such as deploys, and for "did we ever..." or "why did we..." questions. Search for a
    /// few distinctive words. Each match has a session_id and source to pass to
    /// mmr_read_session. Set all=true to search every project.
    #[tool(
        name = "mmr_find",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn find(
        &self,
        Parameters(args): Parameters<FindToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Return a project's session list plus its newest messages across every harness, in one
    /// call and without a model. Use to brief yourself before starting work on a project with
    /// prior history.
    #[tool(
        name = "mmr_context_project",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn context_project(
        &self,
        Parameters(args): Parameters<ContextProjectToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Return the bundled mmr agent skill as JSON text. Use to load full mmr usage guidance
    /// and retrieval patterns into context before deeper history work.
    #[tool(
        name = "mmr_skill_load",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn skill_load(
        &self,
        Parameters(_args): Parameters<EmptyToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let text = run_cli_string(vec!["skill".to_string(), "load".to_string()]).await?;
        Ok(json_tool_result(serde_json::json!({
            "command": "skill/load",
            "text": text,
        })))
    }
}

#[prompt_router]
impl MmrMcpServer {
    /// Retrieve and summarize the previous stable session for continuity.
    #[prompt(name = "mmr_recall_previous_session")]
    async fn recall_previous_session(
        &self,
        Parameters(args): Parameters<RecallPreviousSessionPromptArgs>,
    ) -> GetPromptResult {
        prompt_result(format!(
            "Use the `mmr_recall` tool to retrieve the previous stable session. \
             Arguments: project={project}, source={source}, n={n}, limit={limit}. \
             Then produce a concise continuation brief with concrete session IDs and \
             only cite details present in the tool result.",
            project = display_opt(args.project.as_deref()),
            source = display_opt(args.source.as_deref()),
            n = args.n.unwrap_or(1),
            limit = args.limit.unwrap_or(50)
        ))
    }

    /// Build a compact project context brief from sessions and messages.
    #[prompt(name = "mmr_project_context_brief")]
    async fn project_context_brief(
        &self,
        Parameters(args): Parameters<ProjectContextPromptArgs>,
    ) -> GetPromptResult {
        prompt_result(format!(
            "Use `mmr_context_project` for project={project}, source={source}, limit={limit}. \
             Summarize current project continuity, recent decisions, unresolved risks, and \
             exact sessions that support the brief.",
            project = display_opt(args.project.as_deref()),
            source = display_opt(args.source.as_deref()),
            limit = args.limit.unwrap_or(100)
        ))
    }

    /// Read one session and produce a continuation handoff.
    #[prompt(name = "mmr_session_handoff")]
    async fn session_handoff(
        &self,
        Parameters(args): Parameters<SessionHandoffPromptArgs>,
    ) -> GetPromptResult {
        prompt_result(format!(
            "Use `mmr_read_session` for session_id={session_id}, source={source}, \
             project={project}. Produce a handoff that separates completed work, \
             changed files, verification evidence, and the next safest step.",
            session_id = args.session_id,
            source = display_opt(args.source.as_deref()),
            project = display_opt(args.project.as_deref())
        ))
    }

    /// Search history first, then read the relevant sessions.
    #[prompt(name = "mmr_find_then_read")]
    async fn find_then_read(
        &self,
        Parameters(args): Parameters<FindThenReadPromptArgs>,
    ) -> GetPromptResult {
        prompt_result(format!(
            "Use `mmr_find` with query={query}, project={project}, source={source}. \
             Inspect the strongest matches, then call `mmr_read_session` for the \
             relevant session IDs. Answer with the exact session IDs used and a \
             short explanation of why each session was relevant.",
            query = args.query,
            project = display_opt(args.project.as_deref()),
            source = display_opt(args.source.as_deref())
        ))
    }
}

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for MmrMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("mmr", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "mmr holds this project's earlier agent sessions from Claude Code, Codex, Cursor, \
             Grok and Pi on this machine. Git history records what changed; these sessions \
             record why, what was tried and rejected, and what was left unfinished. Check them \
             before you change code or config, even a one-line change; before you explain why \
             code is the way it is; when the user asks you to resume or refers to earlier work; \
             and before a risky step such as a deploy or migration. Call mmr_find with a \
             distinctive name from the task (a constant, function, file or error name), then \
             mmr_read_session with a match's session_id and source. If a session gives a reason \
             against the change, or a rule it must follow, tell the user. Call \
             mmr_recall to continue the previous session, and mmr_context_project to brief \
             yourself on the project. Omitting project uses the project of the directory the \
             server runs in. Tools return JSON text that matches the mmr CLI.",
        )
    }
}

impl Default for MmrMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EmptyToolArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListProjectsToolArgs {
    /// Narrow to one harness: claude, codex, cursor, grok or pi. Omit to read every harness.
    source: Option<String>,
    /// Maximum projects to return (default 10).
    limit: Option<usize>,
    /// Items to skip, for paging. Pass next_offset from the previous result.
    offset: Option<usize>,
    /// Sort by timestamp (default) or message-count.
    sort_by: Option<String>,
    /// Sort order: desc (default) or asc.
    order: Option<String>,
}

impl ListProjectsToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["list".to_string(), "projects".to_string()]);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        push_opt(&mut args, "--sort-by", self.sort_by);
        push_opt(&mut args, "--order", self.order);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListSessionsToolArgs {
    /// Narrow to one harness: claude, codex, cursor, grok or pi. Omit to read every harness.
    source: Option<String>,
    /// Project path or name, as in a result's project_name. Omit to use the project of the directory the server runs in.
    project: Option<String>,
    /// Use every project instead of one.
    #[serde(default)]
    all: bool,
    /// Maximum sessions to return (default 20).
    limit: Option<usize>,
    /// Items to skip, for paging. Pass next_offset from the previous result.
    offset: Option<usize>,
    /// Sort by timestamp (default) or message-count.
    sort_by: Option<String>,
    /// Sort order: desc (default) or asc.
    order: Option<String>,
}

impl ListSessionsToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["list".to_string(), "sessions".to_string()]);
        push_opt(&mut args, "--project", self.project);
        push_flag(&mut args, "--all", self.all);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        push_opt(&mut args, "--sort-by", self.sort_by);
        push_opt(&mut args, "--order", self.order);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadSessionToolArgs {
    /// Session id, from a mmr_find match, mmr_list_sessions or mmr_recall.
    #[serde(alias = "session")]
    session_id: String,
    /// Narrow to one harness: claude, codex, cursor, grok or pi. Omit to read every harness.
    source: Option<String>,
    /// Project path or name to narrow the lookup, as in a result's project_name.
    project: Option<String>,
    /// Maximum messages to include.
    limit: Option<usize>,
    /// Items to skip, for paging. Pass next_offset from the previous result.
    offset: Option<usize>,
}

impl ReadSessionToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["read".to_string(), "session".to_string(), self.session_id]);
        push_opt(&mut args, "--project", self.project);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadProjectToolArgs {
    /// Narrow to one harness: claude, codex, cursor, grok or pi. Omit to read every harness.
    source: Option<String>,
    /// Project path or name, as in a result's project_name. Omit to use the project of the directory the server runs in.
    project: Option<String>,
    /// Maximum messages to include.
    limit: Option<usize>,
    /// Items to skip, for paging. Pass next_offset from the previous result.
    offset: Option<usize>,
}

impl ReadProjectToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["read".to_string(), "project".to_string()]);
        push_opt(&mut args, "--project", self.project);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecallToolArgs {
    /// How many sessions back: 1 (default) is the previous stable session.
    n: Option<u32>,
    /// Narrow to one harness: claude, codex, cursor, grok or pi. Omit to read every harness.
    source: Option<String>,
    /// Project path or name, as in a result's project_name. Omit to use the project of the directory the server runs in.
    project: Option<String>,
    /// Use every project instead of one.
    #[serde(default)]
    all: bool,
    /// Maximum messages to return (default 50).
    limit: Option<usize>,
    /// Allow the newest session, which may be the caller's own live session.
    #[serde(default)]
    include_newest: bool,
}

impl RecallToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.push("recall".to_string());
        if let Some(n) = self.n {
            args.push(n.to_string());
        }
        push_opt(&mut args, "--project", self.project);
        push_flag(&mut args, "--all", self.all);
        push_opt(&mut args, "--limit", self.limit);
        push_flag(&mut args, "--include-newest", self.include_newest);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindToolArgs {
    /// Text to find in message content; case-insensitive.
    query: String,
    /// Narrow to one harness: claude, codex, cursor, grok or pi. Omit to read every harness.
    source: Option<String>,
    /// Project path or name, as in a result's project_name. Omit to use the project of the directory the server runs in.
    project: Option<String>,
    /// Use every project instead of one.
    #[serde(default)]
    all: bool,
    /// Only search this session id.
    #[serde(alias = "session_id")]
    session: Option<String>,
    /// Maximum matches to return (default 20).
    limit: Option<usize>,
    /// Items to skip, for paging. Pass next_offset from the previous result.
    offset: Option<usize>,
}

impl FindToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["find".to_string(), self.query]);
        push_opt(&mut args, "--project", self.project);
        push_flag(&mut args, "--all", self.all);
        push_opt(&mut args, "--session", self.session);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ContextProjectToolArgs {
    /// Narrow to one harness: claude, codex, cursor, grok or pi. Omit to read every harness.
    source: Option<String>,
    /// Project path or name, as in a result's project_name. Omit to use the project of the directory the server runs in.
    project: Option<String>,
    /// Maximum newest messages to include (default 100).
    limit: Option<usize>,
}

impl ContextProjectToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["context".to_string(), "project".to_string()]);
        push_opt(&mut args, "--project", self.project);
        push_opt(&mut args, "--limit", self.limit);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecallPreviousSessionPromptArgs {
    project: Option<String>,
    source: Option<String>,
    #[serde(default, deserialize_with = "string_or_u32_opt")]
    n: Option<u32>,
    #[serde(default, deserialize_with = "string_or_usize_opt")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ProjectContextPromptArgs {
    project: Option<String>,
    source: Option<String>,
    #[serde(default, deserialize_with = "string_or_usize_opt")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SessionHandoffPromptArgs {
    session_id: String,
    source: Option<String>,
    project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindThenReadPromptArgs {
    query: String,
    project: Option<String>,
    source: Option<String>,
}

async fn run_cli_tool(args: Vec<String>) -> Result<CallToolResult, ErrorData> {
    run_cli_string(args).await.map(text_tool_result)
}

async fn run_cli_string(args: Vec<String>) -> Result<String, ErrorData> {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("mmr".to_string());
    argv.extend(args);
    let cli = Cli::try_parse_from(argv).map_err(|error| {
        ErrorData::invalid_params(
            "invalid mmr CLI arguments",
            Some(Value::String(error.to_string())),
        )
    })?;
    run_cli(cli).await.map_err(|error| {
        let message = error.to_string();
        if message.contains("requires --source") {
            ErrorData::invalid_params(message, None)
        } else {
            ErrorData::internal_error(message, None)
        }
    })
}

fn text_tool_result(text: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(text)])
}

fn json_tool_result(value: Value) -> CallToolResult {
    text_tool_result(value.to_string())
}

fn prompt_result(text: String) -> GetPromptResult {
    GetPromptResult::new(vec![PromptMessage::new_text(PromptMessageRole::User, text)])
}

fn push_source(args: &mut Vec<String>, source: Option<String>) {
    push_opt(args, "--source", source);
}

fn push_opt<T: ToString>(args: &mut Vec<String>, name: &str, value: Option<T>) {
    if let Some(value) = value {
        args.push(name.to_string());
        args.push(value.to_string());
    }
}

fn push_flag(args: &mut Vec<String>, name: &str, value: bool) {
    if value {
        args.push(name.to_string());
    }
}

fn display_opt(value: Option<&str>) -> &str {
    value.unwrap_or("<auto>")
}

fn string_or_usize_opt<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    string_or_number_opt(deserializer, |value| usize::try_from(value).ok())
}

fn string_or_u32_opt<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    string_or_number_opt(deserializer, |value| u32::try_from(value).ok())
}

fn string_or_number_opt<'de, D, T, F>(deserializer: D, convert: F) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    F: Fn(u64) -> Option<T>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(convert)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected non-negative integer")),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
        Some(Value::String(text)) => text
            .parse::<u64>()
            .ok()
            .and_then(convert)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected non-negative integer string")),
        Some(_) => Err(serde::de::Error::custom(
            "expected integer or integer string",
        )),
    }
}
