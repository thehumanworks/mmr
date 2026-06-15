use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
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
    /// Transport to serve MCP over: stdio or streamable HTTP
    #[arg(long, value_enum)]
    pub transport: McpTransportArg,
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

pub async fn run_mcp(args: &McpArgs) -> Result<()> {
    match args.transport {
        McpTransportArg::Stdio => run_stdio().await,
        McpTransportArg::Http => run_http(args.bind, &args.path).await,
    }
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

    /// List known mmr projects with source coverage and recency metadata.
    #[tool(name = "mmr_list_projects")]
    async fn list_projects(
        &self,
        Parameters(args): Parameters<ListProjectsToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// List mmr sessions in a project, source, or all-project scope.
    #[tool(name = "mmr_list_sessions")]
    async fn list_sessions(
        &self,
        Parameters(args): Parameters<ListSessionsToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Read one mmr session by session ID.
    #[tool(name = "mmr_read_session")]
    async fn read_session(
        &self,
        Parameters(args): Parameters<ReadSessionToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Read chronological mmr project history.
    #[tool(name = "mmr_read_project")]
    async fn read_project(
        &self,
        Parameters(args): Parameters<ReadProjectToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Read chronological mmr history for one explicit source.
    #[tool(name = "mmr_read_source")]
    async fn read_source(
        &self,
        Parameters(args): Parameters<ReadSourceToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Retrieve a previous stable session for immediate continuity.
    #[tool(name = "mmr_recall")]
    async fn recall(
        &self,
        Parameters(args): Parameters<RecallToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Search linked normalized mmr events and learned memory.
    #[tool(name = "mmr_find")]
    async fn find(
        &self,
        Parameters(args): Parameters<FindToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let format = args.format.clone();
        let output = run_cli_string(args.into_cli_args()).await?;
        if matches!(format.as_deref(), Some("line")) {
            return Ok(json_tool_result(serde_json::json!({
                "command": "find",
                "format": "line",
                "text": output,
            })));
        }
        Ok(text_tool_result(output))
    }

    /// Produce project-specific context across sources.
    #[tool(name = "mmr_context_project")]
    async fn context_project(
        &self,
        Parameters(args): Parameters<ContextProjectToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Produce source-wide context for one explicit source.
    #[tool(name = "mmr_context_source")]
    async fn context_source(
        &self,
        Parameters(args): Parameters<ContextSourceToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Return project memory-assimilation prompt, runbook, output contract, and evidence.
    #[tool(name = "mmr_assimilate_project")]
    async fn assimilate_project(
        &self,
        Parameters(args): Parameters<AssimilateProjectToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Return source memory-assimilation prompt, runbook, output contract, and evidence.
    #[tool(name = "mmr_assimilate_source")]
    async fn assimilate_source(
        &self,
        Parameters(args): Parameters<AssimilateSourceToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Summarize project history through the configured OpenAI-compatible provider.
    #[tool(name = "mmr_summarize_project")]
    async fn summarize_project(
        &self,
        Parameters(args): Parameters<SummarizeProjectToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_summary_tool(args.output_format.clone(), args.into_cli_args()).await
    }

    /// Summarize one explicit session through the configured OpenAI-compatible provider.
    #[tool(name = "mmr_summarize_session")]
    async fn summarize_session(
        &self,
        Parameters(args): Parameters<SummarizeSessionToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_summary_tool(args.output_format.clone(), args.into_cli_args()).await
    }

    /// Summarize one explicit source through the configured OpenAI-compatible provider.
    #[tool(name = "mmr_summarize_source")]
    async fn summarize_source(
        &self,
        Parameters(args): Parameters<SummarizeSourceToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_summary_tool(args.output_format.clone(), args.into_cli_args()).await
    }

    /// Compact project history with Morph Compact.
    #[tool(name = "mmr_compact_project")]
    async fn compact_project(
        &self,
        Parameters(args): Parameters<CompactProjectToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_compact_tool(args.output_format.clone(), args.into_cli_args()).await
    }

    /// Compact one explicit session with Morph Compact.
    #[tool(name = "mmr_compact_session")]
    async fn compact_session(
        &self,
        Parameters(args): Parameters<CompactSessionToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_compact_tool(args.output_format.clone(), args.into_cli_args()).await
    }

    /// Compact one explicit source with Morph Compact.
    #[tool(name = "mmr_compact_source")]
    async fn compact_source(
        &self,
        Parameters(args): Parameters<CompactSourceToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_compact_tool(args.output_format.clone(), args.into_cli_args()).await
    }

    /// Inspect local project, redaction, source, and sync state.
    #[tool(name = "mmr_status")]
    async fn status(
        &self,
        Parameters(args): Parameters<StatusToolArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run_cli_tool(args.into_cli_args()).await
    }

    /// Return the bundled mmr agent skill as JSON text.
    #[tool(name = "mmr_skill_load")]
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

    /// Run the assimilation handoff and produce evidence-backed memory candidates.
    #[prompt(name = "mmr_memory_assimilation")]
    async fn memory_assimilation(
        &self,
        Parameters(args): Parameters<MemoryAssimilationPromptArgs>,
    ) -> GetPromptResult {
        prompt_result(format!(
            "Use `mmr_assimilate_project` for project={project}, source={source}, \
             evidence_mode={evidence_mode}. Convert the returned evidence bundle into \
             stable, non-sensitive memory candidates. Preserve citations and reject \
             unsupported claims.",
            project = display_opt(args.project.as_deref()),
            source = display_opt(args.source.as_deref()),
            evidence_mode = args.evidence_mode.as_deref().unwrap_or("shared-safe")
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
            "mmr exposes local AI coding session history. Tools return JSON text that matches \
             the mmr CLI contract; prompts describe reusable history-retrieval workflows.",
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
    source: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_by: Option<String>,
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
    source: Option<String>,
    project: Option<String>,
    #[serde(default)]
    all: bool,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_by: Option<String>,
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
    session_id: String,
    source: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
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
    source: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
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
struct ReadSourceToolArgs {
    source: String,
    limit: Option<usize>,
    offset: Option<usize>,
}

impl ReadSourceToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, Some(self.source));
        args.extend(["read".to_string(), "source".to_string()]);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RecallToolArgs {
    n: Option<u32>,
    source: Option<String>,
    project: Option<String>,
    #[serde(default)]
    all: bool,
    limit: Option<usize>,
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
    query: String,
    source: Option<String>,
    project: Option<PathBuf>,
    session: Option<String>,
    role: Option<String>,
    #[serde(default)]
    ignore_case: bool,
    context: Option<usize>,
    format: Option<String>,
}

impl FindToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["find".to_string(), self.query]);
        push_opt(&mut args, "--project", self.project.map(path_to_string));
        push_opt(&mut args, "--session", self.session);
        push_opt(&mut args, "--role", self.role);
        push_flag(&mut args, "--ignore-case", self.ignore_case);
        push_opt(&mut args, "--context", self.context);
        push_opt(&mut args, "--format", self.format);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ContextProjectToolArgs {
    source: Option<String>,
    project: Option<String>,
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
struct ContextSourceToolArgs {
    source: String,
    limit: Option<usize>,
}

impl ContextSourceToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, Some(self.source));
        args.extend(["context".to_string(), "source".to_string()]);
        push_opt(&mut args, "--limit", self.limit);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AssimilateProjectToolArgs {
    source: Option<String>,
    project: Option<PathBuf>,
    evidence_mode: Option<String>,
    #[serde(default)]
    allow_raw_evidence: bool,
}

impl AssimilateProjectToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["assimilate".to_string(), "project".to_string()]);
        push_opt(&mut args, "--project", self.project.map(path_to_string));
        push_opt(&mut args, "--evidence-mode", self.evidence_mode);
        push_flag(&mut args, "--allow-raw-evidence", self.allow_raw_evidence);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AssimilateSourceToolArgs {
    source: String,
    evidence_mode: Option<String>,
    #[serde(default)]
    allow_raw_evidence: bool,
    per_project_limit: Option<usize>,
    since: Option<String>,
}

impl AssimilateSourceToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, Some(self.source));
        args.extend(["assimilate".to_string(), "source".to_string()]);
        push_opt(&mut args, "--evidence-mode", self.evidence_mode);
        push_flag(&mut args, "--allow-raw-evidence", self.allow_raw_evidence);
        push_opt(&mut args, "--per-project-limit", self.per_project_limit);
        push_opt(&mut args, "--since", self.since);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SummarizeProjectToolArgs {
    source: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    instructions: Option<String>,
    model: Option<String>,
    output_format: Option<String>,
}

impl SummarizeProjectToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["summarize".to_string(), "project".to_string()]);
        push_opt(&mut args, "--project", self.project);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        push_summary_runner(&mut args, self.instructions, self.model, self.output_format);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SummarizeSessionToolArgs {
    session_id: String,
    source: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    instructions: Option<String>,
    model: Option<String>,
    output_format: Option<String>,
}

impl SummarizeSessionToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend([
            "summarize".to_string(),
            "session".to_string(),
            self.session_id,
        ]);
        push_opt(&mut args, "--project", self.project);
        push_opt(&mut args, "--limit", self.limit);
        push_opt(&mut args, "--offset", self.offset);
        push_summary_runner(&mut args, self.instructions, self.model, self.output_format);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SummarizeSourceToolArgs {
    source: String,
    instructions: Option<String>,
    model: Option<String>,
    output_format: Option<String>,
}

impl SummarizeSourceToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, Some(self.source));
        args.extend(["summarize".to_string(), "source".to_string()]);
        push_summary_runner(&mut args, self.instructions, self.model, self.output_format);
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CompactProjectToolArgs {
    source: Option<String>,
    project: Option<String>,
    remote: Option<String>,
    query: Option<String>,
    compression_ratio: Option<f32>,
    preserve_recent: Option<u32>,
    no_line_ranges: Option<bool>,
    no_markers: Option<bool>,
    model: Option<String>,
    output_format: Option<String>,
}

impl CompactProjectToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend(["compact".to_string(), "project".to_string()]);
        push_opt(&mut args, "--project", self.project);
        push_opt(&mut args, "--remote", self.remote);
        push_compact_runner(
            &mut args,
            CompactRunnerToolArgs {
                query: self.query,
                compression_ratio: self.compression_ratio,
                preserve_recent: self.preserve_recent,
                no_line_ranges: self.no_line_ranges,
                no_markers: self.no_markers,
                model: self.model,
                output_format: self.output_format,
            },
        );
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CompactSessionToolArgs {
    session_id: String,
    source: Option<String>,
    project: Option<String>,
    remote: Option<String>,
    query: Option<String>,
    compression_ratio: Option<f32>,
    preserve_recent: Option<u32>,
    no_line_ranges: Option<bool>,
    no_markers: Option<bool>,
    model: Option<String>,
    output_format: Option<String>,
}

impl CompactSessionToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.extend([
            "compact".to_string(),
            "session".to_string(),
            self.session_id,
        ]);
        push_opt(&mut args, "--project", self.project);
        push_opt(&mut args, "--remote", self.remote);
        push_compact_runner(
            &mut args,
            CompactRunnerToolArgs {
                query: self.query,
                compression_ratio: self.compression_ratio,
                preserve_recent: self.preserve_recent,
                no_line_ranges: self.no_line_ranges,
                no_markers: self.no_markers,
                model: self.model,
                output_format: self.output_format,
            },
        );
        args
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CompactSourceToolArgs {
    source: String,
    remote: Option<String>,
    query: Option<String>,
    compression_ratio: Option<f32>,
    preserve_recent: Option<u32>,
    no_line_ranges: Option<bool>,
    no_markers: Option<bool>,
    model: Option<String>,
    output_format: Option<String>,
}

impl CompactSourceToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, Some(self.source));
        args.extend(["compact".to_string(), "source".to_string()]);
        push_opt(&mut args, "--remote", self.remote);
        push_compact_runner(
            &mut args,
            CompactRunnerToolArgs {
                query: self.query,
                compression_ratio: self.compression_ratio,
                preserve_recent: self.preserve_recent,
                no_line_ranges: self.no_line_ranges,
                no_markers: self.no_markers,
                model: self.model,
                output_format: self.output_format,
            },
        );
        args
    }
}

struct CompactRunnerToolArgs {
    query: Option<String>,
    compression_ratio: Option<f32>,
    preserve_recent: Option<u32>,
    no_line_ranges: Option<bool>,
    no_markers: Option<bool>,
    model: Option<String>,
    output_format: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusToolArgs {
    source: Option<String>,
    project: Option<PathBuf>,
}

impl StatusToolArgs {
    fn into_cli_args(self) -> Vec<String> {
        let mut args = Vec::new();
        push_source(&mut args, self.source);
        args.push("status".to_string());
        push_opt(&mut args, "--project", self.project.map(path_to_string));
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
struct MemoryAssimilationPromptArgs {
    project: Option<String>,
    source: Option<String>,
    evidence_mode: Option<String>,
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

async fn run_summary_tool(
    output_format: Option<String>,
    args: Vec<String>,
) -> Result<CallToolResult, ErrorData> {
    let output = run_cli_string(args).await?;
    if matches!(output_format.as_deref(), Some("md")) {
        return Ok(json_tool_result(serde_json::json!({
            "command": "summarize",
            "format": "md",
            "text": output,
        })));
    }
    Ok(text_tool_result(output))
}

async fn run_compact_tool(
    output_format: Option<String>,
    args: Vec<String>,
) -> Result<CallToolResult, ErrorData> {
    let output = run_cli_string(args).await?;
    if matches!(output_format.as_deref(), Some("md")) {
        return Ok(json_tool_result(serde_json::json!({
            "command": "compact",
            "format": "md",
            "text": output,
        })));
    }
    Ok(text_tool_result(output))
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

fn push_summary_runner(
    args: &mut Vec<String>,
    instructions: Option<String>,
    model: Option<String>,
    output_format: Option<String>,
) {
    push_opt(args, "--instructions", instructions);
    push_opt(args, "--model", model);
    push_opt(
        args,
        "--output-format",
        Some(output_format.unwrap_or_else(|| "json".to_string())),
    );
}

fn push_compact_runner(args: &mut Vec<String>, runner: CompactRunnerToolArgs) {
    push_opt(args, "--query", runner.query);
    push_opt(args, "--compression-ratio", runner.compression_ratio);
    push_opt(args, "--preserve-recent", runner.preserve_recent);
    push_flag(
        args,
        "--no-line-ranges",
        runner.no_line_ranges.unwrap_or(false),
    );
    push_flag(args, "--no-markers", runner.no_markers.unwrap_or(false));
    push_opt(args, "--model", runner.model);
    push_opt(
        args,
        "--output-format",
        Some(runner.output_format.unwrap_or_else(|| "json".to_string())),
    );
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
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
