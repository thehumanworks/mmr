use std::collections::BTreeMap;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::{Args, Parser, Subcommand, ValueEnum};
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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::cli::{Cli, run_cli};

const PYTHON_BOOTSTRAP_CONTENT: &str = include_str!("../scripts/mmr_rest_mcp_bootstrap.py");
const REST_OPENAPI_CONTENT: &str = include_str!("../docs/api/openapi.json");

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Create or launch a Python FastMCP server generated from the mmr REST OpenAPI document.
    #[command(subcommand)]
    pub command: Option<McpCommand>,
    /// Transport to serve MCP over: stdio or streamable HTTP
    #[arg(long, value_enum)]
    pub transport: Option<McpTransportArg>,
    /// HTTP bind address. Ignored for stdio.
    #[arg(long, default_value = "127.0.0.1:8765")]
    pub bind: SocketAddr,
    /// HTTP mount path. Ignored for stdio.
    #[arg(long, default_value = "/mcp")]
    pub path: String,
}

#[derive(Subcommand, Debug)]
#[clap(rename_all = "kebab-case")]
pub enum McpCommand {
    /// Print, write, dry-run, or launch the Python FastMCP OpenAPI bootstrap.
    PythonBootstrap(PythonBootstrapArgs),
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "kebab-case")]
pub enum McpTransportArg {
    Stdio,
    Http,
}

impl McpTransportArg {
    fn as_env_value(self) -> &'static str {
        match self {
            McpTransportArg::Stdio => "stdio",
            McpTransportArg::Http => "http",
        }
    }
}

#[derive(Args, Debug)]
pub struct PythonBootstrapArgs {
    /// External REST API base URL. Omit to auto-start a loopback mmr REST backing server for --run.
    #[arg(long)]
    pub api_base_url: Option<String>,
    /// External OpenAPI document URL or local JSON file path. Omit to use the auto-started backing server for --run.
    #[arg(long)]
    pub openapi_url: Option<String>,
    /// Environment variable name that contains the optional REST bearer token.
    #[arg(long, default_value = "API_TOKEN")]
    pub api_token_env: String,
    /// Python executable used for --run.
    #[arg(long, default_value = "python3")]
    pub python: PathBuf,
    /// Existing bootstrap script to launch with --run. Defaults to the bundled script.
    #[arg(long)]
    pub script: Option<PathBuf>,
    /// MCP transport used by the Python FastMCP server when --run is set.
    #[arg(long, value_enum, default_value = "stdio")]
    pub transport: McpTransportArg,
    /// HTTP host used by the Python FastMCP server when --transport http and --run are set.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// HTTP port used by the Python FastMCP server when --transport http and --run are set.
    #[arg(long, default_value_t = 8766)]
    pub port: u16,
    /// Print the bundled Python bootstrap to stdout. This is the default action.
    #[arg(long)]
    pub print: bool,
    /// Write the bundled Python bootstrap to this path.
    #[arg(long, value_name = "PATH")]
    pub write: Option<PathBuf>,
    /// Print deterministic launch metadata as JSON without importing FastMCP or starting Python.
    #[arg(long)]
    pub dry_run: bool,
    /// Launch the Python FastMCP bootstrap. Requires Python dependencies: fastmcp and httpx.
    #[arg(long)]
    pub run: bool,
    /// Do not auto-start the internal mmr REST backing server for --run.
    #[arg(long)]
    pub no_auto_rest: bool,
}

#[derive(Serialize)]
struct PythonBootstrapWriteResponse {
    command: &'static str,
    action: &'static str,
    path: String,
    bytes: usize,
}

#[derive(Serialize)]
struct PythonBootstrapLaunchPlan {
    command: &'static str,
    action: &'static str,
    python: String,
    script: String,
    argv: Vec<String>,
    env: BTreeMap<&'static str, String>,
    rest_backing: PythonBootstrapRestBackingPlan,
}

#[derive(Serialize)]
struct PythonBootstrapRestBackingPlan {
    mode: &'static str,
    api_base_url: String,
    openapi_url: String,
}

pub async fn run_mcp(args: &McpArgs, pretty: bool) -> Result<String> {
    if let Some(command) = &args.command {
        return match command {
            McpCommand::PythonBootstrap(bootstrap_args) => {
                run_python_bootstrap(bootstrap_args, pretty).await
            }
        };
    }

    match args.transport {
        Some(McpTransportArg::Stdio) => run_stdio().await?,
        Some(McpTransportArg::Http) => run_http(args.bind, &args.path).await?,
        None => bail!(
            "missing --transport <stdio|http>; use `mmr mcp --transport stdio`, \
             `mmr mcp --transport http`, or `mmr mcp python-bootstrap`"
        ),
    }
    Ok(String::new())
}

async fn run_python_bootstrap(args: &PythonBootstrapArgs, pretty: bool) -> Result<String> {
    let actions = usize::from(args.print)
        + usize::from(args.write.is_some())
        + usize::from(args.dry_run)
        + usize::from(args.run);
    if actions > 1 {
        bail!("choose only one python-bootstrap action: --print, --write, --dry-run, or --run");
    }

    if let Some(path) = &args.write {
        fs::write(path, PYTHON_BOOTSTRAP_CONTENT)
            .with_context(|| format!("write Python MCP bootstrap to {}", path.display()))?;
        return serialize_mcp_response(
            &PythonBootstrapWriteResponse {
                command: "mcp/python-bootstrap",
                action: "write",
                path: path.display().to_string(),
                bytes: PYTHON_BOOTSTRAP_CONTENT.len(),
            },
            pretty,
        );
    }

    if args.dry_run {
        return serialize_mcp_response(&python_bootstrap_launch_plan(args, "dry-run"), pretty);
    }

    if args.run {
        run_python_bootstrap_process(args).await?;
        return Ok(String::new());
    }

    Ok(PYTHON_BOOTSTRAP_CONTENT.to_string())
}

fn python_bootstrap_launch_plan(
    args: &PythonBootstrapArgs,
    action: &'static str,
) -> PythonBootstrapLaunchPlan {
    let script = python_bootstrap_script_path(args);
    let rest_backing = python_bootstrap_rest_backing_plan(args);
    PythonBootstrapLaunchPlan {
        command: "mcp/python-bootstrap",
        action,
        python: args.python.display().to_string(),
        script: script.display().to_string(),
        argv: vec![
            args.python.display().to_string(),
            script.display().to_string(),
        ],
        env: python_bootstrap_env(args, &rest_backing.api_base_url, &rest_backing.openapi_url),
        rest_backing,
    }
}

fn python_bootstrap_rest_backing_plan(
    args: &PythonBootstrapArgs,
) -> PythonBootstrapRestBackingPlan {
    let auto = should_auto_start_rest(args);
    let api_base_url = if auto {
        "<auto-started-loopback>".to_string()
    } else {
        external_api_base_url(args)
    };
    let openapi_url = if auto {
        "<auto-started-loopback>/openapi.json".to_string()
    } else {
        external_openapi_url(args, &api_base_url)
    };
    PythonBootstrapRestBackingPlan {
        mode: if auto { "auto" } else { "external" },
        api_base_url,
        openapi_url,
    }
}

fn should_auto_start_rest(args: &PythonBootstrapArgs) -> bool {
    (args.run || args.dry_run)
        && !args.no_auto_rest
        && args.api_base_url.is_none()
        && args.openapi_url.is_none()
}

fn external_api_base_url(args: &PythonBootstrapArgs) -> String {
    args.api_base_url
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:8765".to_string())
}

fn external_openapi_url(args: &PythonBootstrapArgs, api_base_url: &str) -> String {
    args.openapi_url
        .clone()
        .unwrap_or_else(|| format!("{}/openapi.json", api_base_url.trim_end_matches('/')))
}

fn python_bootstrap_env(
    args: &PythonBootstrapArgs,
    api_base_url: &str,
    openapi_url: &str,
) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("API_BASE_URL", api_base_url.to_string()),
        ("OPENAPI_URL", openapi_url.to_string()),
        ("API_TOKEN_ENV", args.api_token_env.clone()),
        ("MCP_TRANSPORT", args.transport.as_env_value().to_string()),
        ("MCP_HOST", args.host.clone()),
        ("MCP_PORT", args.port.to_string()),
    ])
}

fn python_bootstrap_script_path(args: &PythonBootstrapArgs) -> PathBuf {
    args.script.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "mmr-{}-rest-mcp-bootstrap.py",
            env!("CARGO_PKG_VERSION")
        ))
    })
}

fn ensure_python_bootstrap_script(args: &PythonBootstrapArgs) -> Result<PathBuf> {
    if let Some(path) = &args.script {
        return Ok(path.clone());
    }
    let path = python_bootstrap_script_path(args);
    fs::write(&path, PYTHON_BOOTSTRAP_CONTENT)
        .with_context(|| format!("write bundled Python MCP bootstrap to {}", path.display()))?;
    Ok(path)
}

async fn run_python_bootstrap_process(args: &PythonBootstrapArgs) -> Result<()> {
    let script = ensure_python_bootstrap_script(args)?;
    if should_auto_start_rest(args) {
        let rest_backing = RestBackingServer::start().await?;
        let api_base_url = rest_backing.api_base_url();
        let openapi_url = format!("{api_base_url}/openapi.json");
        eprintln!("mmr REST backing server listening on {api_base_url}");
        let result = run_python_bootstrap_command(args, &script, &api_base_url, &openapi_url).await;
        let shutdown = rest_backing.shutdown().await;
        result?;
        shutdown?;
        return Ok(());
    }

    let api_base_url = external_api_base_url(args);
    let openapi_url = external_openapi_url(args, &api_base_url);
    run_python_bootstrap_command(args, &script, &api_base_url, &openapi_url).await
}

async fn run_python_bootstrap_command(
    args: &PythonBootstrapArgs,
    script: &PathBuf,
    api_base_url: &str,
    openapi_url: &str,
) -> Result<()> {
    let mut command = Command::new(&args.python);
    command
        .arg(script)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (key, value) in python_bootstrap_env(args, api_base_url, openapi_url) {
        command.env(key, value);
    }

    let status = command.status().await.with_context(|| {
        format!(
            "failed to start Python MCP bootstrap with {}; ensure Python is installed and install \
             dependencies with `python3 -m pip install fastmcp httpx`",
            args.python.display()
        )
    })?;
    if !status.success() {
        bail!(
            "Python MCP bootstrap exited with {status}. Install dependencies with \
             `python3 -m pip install fastmcp httpx` and verify API_BASE_URL/OPENAPI_URL."
        );
    }
    Ok(())
}

struct RestBackingServer {
    local_addr: SocketAddr,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<Result<()>>,
}

impl RestBackingServer {
    async fn start() -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind internal mmr REST backing server")?;
        let local_addr = listener.local_addr()?;
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, rest_backing_router())
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .context("serve internal mmr REST backing server")
        });
        Ok(Self {
            local_addr,
            shutdown,
            task,
        })
    }

    fn api_base_url(&self) -> String {
        format!("http://{}", self.local_addr)
    }

    async fn shutdown(self) -> Result<()> {
        let _ = self.shutdown.send(());
        self.task
            .await
            .context("join internal mmr REST backing server")??;
        Ok(())
    }
}

fn rest_backing_router() -> Router {
    Router::new()
        .route("/openapi.json", get(rest_openapi))
        .route("/v1/status", get(rest_status))
        .route("/v1/projects", get(rest_list_projects))
        .route("/v1/sessions", get(rest_list_sessions))
        .route(
            "/v1/sessions/{session_id}/messages",
            get(rest_read_session_messages),
        )
        .route("/v1/messages", get(rest_read_messages))
        .route("/v1/recall", get(rest_recall))
        .route("/v1/find", get(rest_find))
        .route("/v1/context/project", get(rest_context_project))
        .route("/v1/context/source", get(rest_context_source))
        .route(
            "/v1/redactions/events/{event_id}",
            get(rest_redaction_explain),
        )
        .route("/v1/sync-dry-runs", post(rest_sync_dry_run))
}

#[derive(Debug, Deserialize, Default)]
struct RestQueryParams {
    source: Option<String>,
    project: Option<String>,
    all: Option<bool>,
    limit: Option<usize>,
    offset: Option<usize>,
    sort_by: Option<String>,
    order: Option<String>,
    scope: Option<String>,
    query: Option<String>,
    session: Option<String>,
    role: Option<String>,
    event_type: Option<String>,
    ignore_case: Option<bool>,
    context: Option<usize>,
    n: Option<u32>,
    include_newest: Option<bool>,
}

async fn rest_openapi() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        REST_OPENAPI_CONTENT,
    )
        .into_response()
}

async fn rest_status(Query(query): Query<RestQueryParams>) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.push("status".to_string());
    push_opt(&mut args, "--project", query.project);
    rest_cli_response(args).await
}

async fn rest_list_projects(Query(query): Query<RestQueryParams>) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.extend(["list".to_string(), "projects".to_string()]);
    push_opt(&mut args, "--limit", query.limit);
    push_opt(&mut args, "--offset", query.offset);
    push_opt(&mut args, "--sort-by", query.sort_by);
    push_opt(&mut args, "--order", query.order);
    rest_cli_response(args).await
}

async fn rest_list_sessions(Query(query): Query<RestQueryParams>) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.extend(["list".to_string(), "sessions".to_string()]);
    push_opt(&mut args, "--project", query.project);
    push_flag(&mut args, "--all", query.all.unwrap_or(false));
    push_opt(&mut args, "--limit", query.limit);
    push_opt(&mut args, "--offset", query.offset);
    push_opt(&mut args, "--sort-by", query.sort_by);
    push_opt(&mut args, "--order", query.order);
    rest_cli_response(args).await
}

async fn rest_read_session_messages(
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<RestQueryParams>,
) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.extend(["read".to_string(), "session".to_string(), session_id]);
    push_opt(&mut args, "--project", query.project);
    push_opt(&mut args, "--limit", query.limit);
    push_opt(&mut args, "--offset", query.offset);
    rest_cli_response(args).await
}

async fn rest_read_messages(Query(query): Query<RestQueryParams>) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source.clone());
    match query.scope.as_deref() {
        Some("source") => {
            args.extend(["read".to_string(), "source".to_string()]);
        }
        Some("project") => {
            args.extend(["read".to_string(), "project".to_string()]);
            push_opt(&mut args, "--project", query.project);
        }
        _ => return rest_bad_request("scope must be 'project' or 'source'"),
    }
    push_opt(&mut args, "--limit", query.limit);
    push_opt(&mut args, "--offset", query.offset);
    rest_cli_response(args).await
}

async fn rest_recall(Query(query): Query<RestQueryParams>) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.push("recall".to_string());
    if let Some(n) = query.n {
        args.push(n.to_string());
    }
    push_opt(&mut args, "--project", query.project);
    push_flag(&mut args, "--all", query.all.unwrap_or(false));
    push_opt(&mut args, "--limit", query.limit);
    push_opt(&mut args, "--offset", query.offset);
    push_flag(
        &mut args,
        "--include-newest",
        query.include_newest.unwrap_or(false),
    );
    rest_cli_response(args).await
}

async fn rest_find(Query(query): Query<RestQueryParams>) -> Response {
    let Some(search_query) = query.query else {
        return rest_bad_request("query is required");
    };
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.extend(["find".to_string(), search_query]);
    push_opt(&mut args, "--project", query.project);
    push_opt(&mut args, "--session", query.session);
    push_opt(&mut args, "--role", query.role);
    push_opt(&mut args, "--event-type", query.event_type);
    push_flag(
        &mut args,
        "--ignore-case",
        query.ignore_case.unwrap_or(false),
    );
    push_opt(&mut args, "--context", query.context);
    rest_cli_response(args).await
}

async fn rest_context_project(Query(query): Query<RestQueryParams>) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.extend(["context".to_string(), "project".to_string()]);
    push_opt(&mut args, "--project", query.project);
    push_opt(&mut args, "--limit", query.limit);
    rest_cli_response(args).await
}

async fn rest_context_source(Query(query): Query<RestQueryParams>) -> Response {
    let mut args = Vec::new();
    push_source(&mut args, query.source);
    args.extend(["context".to_string(), "source".to_string()]);
    push_opt(&mut args, "--limit", query.limit);
    rest_cli_response(args).await
}

async fn rest_redaction_explain(AxumPath(event_id): AxumPath<String>) -> Response {
    rest_cli_response(vec!["redact".to_string(), "explain".to_string(), event_id]).await
}

async fn rest_sync_dry_run(Json(body): Json<Value>) -> Response {
    let project = body
        .get("project")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut args = vec!["sync".to_string(), "--dry-run".to_string()];
    push_opt(&mut args, "--project", project);
    rest_cli_response(args).await
}

async fn rest_cli_response(args: Vec<String>) -> Response {
    match run_cli_anyhow(args).await {
        Ok(output) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            output,
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

fn rest_bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": message.to_string()})),
    )
        .into_response()
}

async fn run_cli_anyhow(args: Vec<String>) -> Result<String> {
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push("mmr".to_string());
    argv.extend(args);
    let cli = Cli::try_parse_from(argv).context("parse mmr REST backing CLI arguments")?;
    run_cli(cli).await
}

fn serialize_mcp_response<T: Serialize>(value: &T, pretty: bool) -> Result<String> {
    if pretty {
        Ok(serde_json::to_string_pretty(value)?)
    } else {
        Ok(serde_json::to_string(value)?)
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
