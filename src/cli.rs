mod find;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::messages::service::{MessageQueryOptions, QueryService, SessionSelectionError};
use crate::peer::{
    PEER_PROTOCOL_VERSION, PeerProjectIdentity, PeerStatusResponse, PeerTeleportPackRequest,
    peer_status, run_peer_json,
};
use crate::teleport::{
    ApplyOptions, PackOptions, ReadOptions, ReceiveOptions, SendOptions, SendTransport, ServeError,
    ServeOptions, TeleportBundleFile, TeleportFailure, TeleportFidelity, TeleportOutputFormat,
    TeleportStatus, apply_bundle, pack_session, read_bundle, receive_bundle, send_session,
    serve_session, verify_artifact_hashes, write_bundle,
};
use crate::types::{ApiMessage, ApiMessagesResponse, SortBy, SortOptions, SortOrder, SourceFilter};

#[derive(Debug, Clone)]
pub struct CliFailure {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliFailure {
    pub fn new(exit_code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    pub fn from_teleport(failure: crate::teleport::TeleportFailure, pretty: bool) -> Result<Self> {
        Ok(Self {
            exit_code: failure.exit_code,
            stdout: failure.to_stdout_json(pretty)?,
            stderr: failure.message,
        })
    }
}

impl std::fmt::Display for CliFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.stderr)
    }
}

impl std::error::Error for CliFailure {}

const ENV_AUTO_DISCOVER_PROJECT: &str = "MMR_AUTO_DISCOVER_PROJECT";

#[derive(Debug, Clone, Copy)]
struct BundledSkillFile {
    relative_path: &'static str,
    contents: &'static str,
}

const BUNDLED_MMR_SKILL_FILES: &[BundledSkillFile] = &[
    BundledSkillFile {
        relative_path: "SKILL.md",
        contents: include_str!("../.agents/skills/mmr/SKILL.md"),
    },
    BundledSkillFile {
        relative_path: "session-mining/SKILL.md",
        contents: include_str!("../.agents/skills/mmr/session-mining/SKILL.md"),
    },
    BundledSkillFile {
        relative_path: "session-mining/references/extraction-jq-patterns.md",
        contents: include_str!(
            "../.agents/skills/mmr/session-mining/references/extraction-jq-patterns.md"
        ),
    },
    BundledSkillFile {
        relative_path: "session-mining/references/session-retrieval-patterns.md",
        contents: include_str!(
            "../.agents/skills/mmr/session-mining/references/session-retrieval-patterns.md"
        ),
    },
];

#[derive(Parser, Debug)]
#[command(
    name = "mmr",
    version = env!("CARGO_PKG_VERSION"),
    about = "Fetch a project's earlier agent sessions from Claude Code, Codex, Cursor, Grok and Pi history",
    after_help = "Examples:\n  mmr find \"retry limit\"                 Search this project's history, newest first\n  mmr --source codex read session <id>   Read one session (each find match prints this)\n  mmr list sessions                      List this project's sessions, newest first\n  mmr recall                             Read the previous session in this project\n  mmr read project --limit 50            Read this project's history, 50 messages a page\n  mmr context project --limit 100        Session list plus bounded messages, in one call\n  mmr share session latest --to user@host\n  mmr import session --from mini --session latest --project /path/to/project --read-only\n  mmr skill load\n  mmr mcp --transport stdio\n\nScope:\n  Project commands use the current directory's project unless you pass --project, or --all where offered. Every command reads each harness's own history files, so nothing needs to be set up first. --source narrows a command to one harness.\n\nOutput:\n  Commands print JSON on stdout; add --pretty to indent it. `mmr mcp --transport stdio` reserves stdout for MCP protocol frames."
)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Only read one harness: claude, codex, cursor, grok or pi (omit to read all)
    #[arg(long, global = true, value_enum)]
    pub source: Option<SourceFilter>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List known projects or sessions
    List(ListArgs),
    /// Find text in project history across every harness; no setup needed
    Find(find::FindArgs),
    /// Retrieve a previous stable session for immediate continuity
    Recall(RecallArgs),
    /// Read raw session or project history
    Read(ReadArgs),
    /// Collect a project's session list and newest messages for an agent
    Context(ContextArgs),
    /// Load or install the bundled mmr agent skill
    Skill(SkillArgs),
    /// Import session or bundle material into this machine
    Import(ImportArgs),
    /// Share one selected session from this machine
    Share(ShareArgs),
    /// Run mmr as a Model Context Protocol server
    Mcp(crate::mcp::McpArgs),
    /// Implementation-facing SSH peer protocol
    #[command(hide = true)]
    Peer(PeerArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    #[command(subcommand)]
    command: ListCommand,
}

#[derive(Subcommand, Debug)]
pub enum ListCommand {
    /// List known projects with source coverage and recency metadata
    Projects(ListProjectsArgs),
    /// List sessions in a scope, defaulting to the cwd project
    Sessions(ListSessionsArgs),
}

#[derive(Args, Debug)]
pub struct ListProjectsArgs {
    /// Maximum number of projects to return
    #[arg(long, default_value_t = 10)]
    limit: usize,
    /// Number of projects to skip
    #[arg(long, default_value_t = 0)]
    offset: usize,
    /// Sort projects by
    #[arg(short = 's', long, default_value = "timestamp")]
    sort_by: SortBy,
    /// Sort order: asc or desc
    #[arg(short = 'o', long, default_value = "desc")]
    order: SortOrder,
}

#[derive(Args, Debug)]
pub struct ListSessionsArgs {
    /// Project name or path
    #[arg(long)]
    project: Option<String>,
    /// Return sessions across all projects instead of the auto-discovered cwd project
    #[arg(long)]
    all: bool,
    /// Maximum number of sessions to return
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Number of sessions to skip
    #[arg(long, default_value_t = 0)]
    offset: usize,
    /// Sort sessions by
    #[arg(short = 's', long, default_value = "timestamp")]
    sort_by: SortBy,
    /// Sort order: asc or desc
    #[arg(short = 'o', long, default_value = "desc")]
    order: SortOrder,
}

#[derive(Args, Debug)]
pub struct RecallArgs {
    /// How many sessions back to read (1 = the previous stable session)
    #[arg(value_name = "N", default_value_t = 1)]
    n: u32,
    /// Project name or path (recency is computed within this scope)
    #[arg(long)]
    project: Option<String>,
    /// Compute recency across all projects instead of the auto-discovered cwd project
    #[arg(long)]
    all: bool,
    /// Maximum number of messages to return
    #[arg(long, default_value_t = 50)]
    limit: usize,
    /// Number of sorted messages to skip
    #[arg(long, default_value_t = 0)]
    offset: usize,
    /// Make the newest (assumed-live, age 0) session addressable
    #[arg(long)]
    include_newest: bool,
}

#[derive(Args, Debug)]
pub struct ReadArgs {
    #[command(subcommand)]
    command: ReadCommand,
}

#[derive(Subcommand, Debug)]
pub enum ReadCommand {
    /// Read one explicit session by ID
    Session(ReadSessionArgs),
    /// Read project-scoped history across all sources
    Project(ReadProjectArgs),
}

#[derive(Args, Debug)]
pub struct ReadSessionArgs {
    /// Session ID to read
    session_id: String,
    /// Project name or path
    #[arg(long)]
    project: Option<String>,
    /// Maximum number of messages to return
    #[arg(long)]
    limit: Option<usize>,
    /// Number of sorted messages to skip
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

#[derive(Args, Debug)]
pub struct ReadProjectArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long)]
    project: Option<String>,
    /// Maximum number of messages to return
    #[arg(long)]
    limit: Option<usize>,
    /// Number of sorted messages to skip
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

#[derive(Args, Debug)]
pub struct ContextArgs {
    #[command(subcommand)]
    command: ContextCommand,
}

#[derive(Subcommand, Debug)]
pub enum ContextCommand {
    /// Produce project-specific context across all sources
    Project(ContextProjectArgs),
}

#[derive(Args, Debug)]
pub struct ContextProjectArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long)]
    project: Option<String>,
    /// Maximum number of recent messages to include
    #[arg(long, default_value_t = 100)]
    limit: usize,
}

#[derive(Args, Debug)]
pub struct SkillArgs {
    #[command(subcommand)]
    command: SkillCommand,
}

#[derive(Subcommand, Debug)]
pub enum SkillCommand {
    /// Print the bundled mmr skill to stdout for immediate agent use
    Load,
    /// Install the bundled mmr skill, replacing any existing target directory
    Install(SkillInstallArgs),
}

#[derive(Args, Debug)]
pub struct SkillInstallArgs {
    /// Install into .agents/skills/mmr under the current directory instead of ~/.agents/skills/mmr
    #[arg(long)]
    local: bool,
}

#[derive(Args, Debug)]
pub struct PeerArgs {
    #[command(subcommand)]
    command: PeerCommand,
}

#[derive(Subcommand, Debug)]
pub enum PeerCommand {
    /// Report local or remote peer protocol capabilities
    Status(PeerStatusArgs),
    /// Hidden JSON-over-stdin native teleport pack endpoint
    #[command(name = "teleport-pack", hide = true)]
    TeleportPack(PeerRequestArgs),
}

#[derive(Args, Debug)]
pub struct PeerStatusArgs {
    /// Query this explicit SSH target
    #[arg(long)]
    host: Option<String>,
    /// Emit JSON; accepted for remote protocol stability
    #[arg(long, hide = true)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct PeerRequestArgs {
    /// Read request JSON from stdin when set to '-'
    #[arg(long = "request-json")]
    request_json: String,
}

#[derive(Args, Debug)]
pub struct ImportArgs {
    #[command(subcommand)]
    command: ImportCommand,
}

#[derive(Subcommand, Debug)]
pub enum ImportCommand {
    /// Pull a selected native session bundle from an SSH peer
    Session(ImportSessionArgs),
    /// Import a local, inbox, stdin, or one-shot HTTP bundle locator
    Bundle(ImportBundleArgs),
}

#[derive(Args, Debug)]
#[command(
    after_help = "Examples:\n  mmr import session --from mini --session latest --project /path/to/project --read-only\n  mmr import session --from user@host:22 --session sess-abc --project /path/to/project --apply"
)]
pub struct ImportSessionArgs {
    /// SSH source target (host, user@host, user@host:port, or ssh://user@host:port)
    #[arg(long = "from")]
    from: String,
    /// Session ID to pull; use literal 'latest' or omit for latest session in scope
    #[arg(long)]
    session: Option<String>,
    /// Select the latest session in scope
    #[arg(long)]
    latest: bool,
    /// Project name or path
    #[arg(long)]
    project: Option<String>,
    /// Read/cache without applying native provider files (default)
    #[arg(long)]
    read_only: bool,
    /// Explicitly restore native provider files after pulling
    #[arg(long)]
    apply: bool,
    /// Replace existing native files (requires --apply)
    #[arg(long)]
    force: bool,
}

#[derive(Args, Debug)]
#[command(
    after_help = "Examples:\n  mmr import bundle ./handoff.mmr --read-only\n  mmr import bundle mmtp://100.x.x.x:PORT/TOKEN --apply --project /path/to/project\n  mmr import bundle --to - --apply"
)]
pub struct ImportBundleArgs {
    /// Bundle path, inbox directory, stdin marker '-', or HTTP locator
    #[arg(value_name = "LOCATOR")]
    locator: Option<String>,
    /// Bundle path, inbox directory, stdin marker '-', or HTTP locator
    #[arg(long)]
    to: Option<String>,
    /// Read/cache without applying native provider files (default)
    #[arg(long)]
    read_only: bool,
    /// Explicitly restore native provider files
    #[arg(long)]
    apply: bool,
    /// Target project path override for apply
    #[arg(long)]
    project: Option<String>,
    /// Replace existing native files (requires --apply)
    #[arg(long)]
    force: bool,
    /// Output format for --read-only
    #[arg(
        short = 'O',
        long = "output-format",
        value_enum,
        default_value = "json"
    )]
    output_format: BundleOutputFormatArg,
    /// Show what would be read/applied without writing files or downloading
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args, Debug)]
pub struct ShareArgs {
    #[command(subcommand)]
    command: ShareCommand,
}

#[derive(Subcommand, Debug)]
pub enum ShareCommand {
    /// Share one native session bundle over SSH, file inbox, or one-shot HTTP
    Session(ShareSessionArgs),
}

#[derive(Args, Debug)]
#[command(
    after_help = "Examples:\n  mmr share session latest --to user@host\n  mmr share session sess-abc --to file:///Users/me/Sync/mmr-inbox\n  mmr share session latest --via http --bind 100.x.x.x:0"
)]
pub struct ShareSessionArgs {
    /// Session ID to share; use literal 'latest' or omit for latest session in scope
    #[arg(value_name = "SESSION")]
    selector: Option<String>,
    /// Session ID to share; use literal 'latest' for the latest session in scope
    #[arg(long)]
    session: Option<String>,
    /// Select the latest session in scope
    #[arg(long)]
    latest: bool,
    /// Project name or path
    #[arg(long)]
    project: Option<String>,
    /// SSH destination, file inbox URL, or HTTP bind address when --via http
    #[arg(long)]
    to: Option<String>,
    /// Transport selector (default: auto; inferred from --to)
    #[arg(long = "via", value_enum)]
    via: Option<ShareTransportArg>,
    /// Bind address host:port for one-shot HTTP
    #[arg(long)]
    bind: Option<String>,
    /// Seconds to wait for one successful HTTP download before exiting
    #[arg(long, default_value_t = 600)]
    timeout: u64,
    /// Show planned SSH/file steps without writing files or contacting remote hosts
    #[arg(long)]
    dry_run: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "kebab-case")]
enum ShareTransportArg {
    Auto,
    Ssh,
    Http,
    File,
}

impl From<ShareTransportArg> for SendTransport {
    fn from(value: ShareTransportArg) -> Self {
        match value {
            ShareTransportArg::Auto => SendTransport::Auto,
            ShareTransportArg::Ssh => SendTransport::Ssh,
            ShareTransportArg::File => SendTransport::File,
            ShareTransportArg::Http => SendTransport::Auto,
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
enum BundleOutputFormatArg {
    #[default]
    Json,
    Md,
}

impl From<BundleOutputFormatArg> for TeleportOutputFormat {
    fn from(value: BundleOutputFormatArg) -> Self {
        match value {
            BundleOutputFormatArg::Json => TeleportOutputFormat::Json,
            BundleOutputFormatArg::Md => TeleportOutputFormat::Md,
        }
    }
}

pub async fn run_cli(cli: Cli) -> Result<String> {
    let source_filter = cli.source;
    let pretty = cli.pretty;
    let load_service = || QueryService::load_filtered(source_filter);
    match cli.command {
        Commands::Mcp(args) => crate::mcp::run_mcp(&args, pretty).await,
        Commands::Skill(args) => skill_command_response(&args, pretty),
        Commands::Import(args) => import_command_response(&args, source_filter, pretty),
        Commands::Share(args) => share_command_response(&args, source_filter, pretty),
        Commands::Peer(args) => peer_command_response(&args, source_filter, pretty),
        Commands::Find(args) => find::find_output(args, &load_service()?, source_filter, pretty),
        Commands::List(args) => {
            list_command_response(&load_service()?, args, source_filter, pretty)
        }
        Commands::Recall(args) => recall_response(&load_service()?, args, source_filter, pretty),
        Commands::Read(args) => {
            read_command_response(&load_service()?, args, source_filter, pretty)
        }
        Commands::Context(args) => {
            context_command_response(&load_service()?, args, source_filter, pretty)
        }
    }
}

fn list_command_response(
    service: &QueryService,
    args: ListArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        ListCommand::Projects(args) => serialize(
            &service.projects(
                source_filter,
                Some(args.limit),
                args.offset,
                SortOptions::new(args.sort_by, args.order),
            ),
            pretty,
        ),
        ListCommand::Sessions(args) => serialize(
            &service.sessions(
                effective_project_scope(args.project, args.all).as_deref(),
                source_filter,
                Some(args.limit),
                args.offset,
                SortOptions::new(args.sort_by, args.order),
            )?,
            pretty,
        ),
    }
}

fn read_command_response(
    service: &QueryService,
    args: ReadArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        ReadCommand::Session(args) => read_session_response(service, args, source_filter, pretty),
        ReadCommand::Project(args) => read_project_response(service, args, source_filter, pretty),
    }
}

fn read_session_response(
    service: &QueryService,
    args: ReadSessionArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    if args.project.is_none() && source_filter.is_none() {
        eprintln!("hint: searching all sources for session; pass --source to narrow the search");
    }
    let response = service.messages(
        &[args.session_id],
        args.project.as_deref(),
        source_filter,
        MessageQueryOptions::new(args.limit, args.offset, oldest_first()),
    )?;
    serialize(&response, pretty)
}

fn read_project_response(
    service: &QueryService,
    args: ReadProjectArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let mut response = match args.project.as_deref() {
        Some(project) => service.messages(
            &[],
            Some(project),
            source_filter,
            MessageQueryOptions::new(args.limit, args.offset, oldest_first()),
        )?,
        None => read_cwd_project_messages(service, source_filter, args.limit, args.offset)?,
    };
    if response.next_page {
        response.next_command = Some(build_next_read_project_command(
            source_filter,
            args.project.as_deref(),
            args.limit,
            response.next_offset as usize,
        ));
    }
    serialize(&response, pretty)
}

fn read_cwd_project_messages(
    service: &QueryService,
    source_filter: Option<SourceFilter>,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiMessagesResponse> {
    let (codex_path, claude_name) =
        resolve_project_from_cwd().context("could not get current directory")?;
    let cursor_name = claude_name.clone();
    let mut messages: Vec<ApiMessage> = Vec::new();
    let mut total_messages = 0;
    for (source, project) in [
        (SourceFilter::Codex, codex_path.as_str()),
        (SourceFilter::Claude, claude_name.as_str()),
        (SourceFilter::Cursor, cursor_name.as_str()),
        (SourceFilter::Grok, codex_path.as_str()),
        (SourceFilter::Pi, codex_path.as_str()),
    ] {
        if source_filter.is_none() || source_filter == Some(source) {
            let response = service.messages(
                &[],
                Some(project),
                Some(source),
                MessageQueryOptions::new(None, 0, oldest_first()),
            )?;
            total_messages += response.total_messages;
            messages.extend(response.messages);
        }
    }
    messages.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    let total = messages.len();
    let paged = paginate(messages, limit, offset);
    let next_offset = offset.saturating_add(paged.len()) as i64;
    Ok(ApiMessagesResponse {
        messages: paged,
        total_messages: total_messages.max(total as i64),
        next_page: next_offset < total as i64,
        next_offset,
        next_command: None,
        session_selection: None,
    })
}

fn paginate<T>(items: Vec<T>, limit: Option<usize>, offset: usize) -> Vec<T> {
    let iter = items.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

fn oldest_first() -> SortOptions {
    SortOptions::new(SortBy::Timestamp, SortOrder::Asc)
}

fn build_next_read_project_command(
    source_filter: Option<SourceFilter>,
    project: Option<&str>,
    limit: Option<usize>,
    next_offset: usize,
) -> String {
    let mut parts = vec!["mmr".to_string()];
    if let Some(source) = source_filter {
        parts.push(format!("--source {}", source.as_str()));
    }
    parts.push("read project".to_string());
    if let Some(project) = project {
        parts.push("--project".to_string());
        parts.push(shell_quote(project));
    }
    if let Some(limit) = limit {
        parts.push(format!("--limit {limit}"));
    }
    parts.push(format!("--offset {next_offset}"));
    parts.join(" ")
}

#[derive(Debug, Serialize)]
struct ContextResponse {
    command: String,
    scope: String,
    source: Option<String>,
    project: Option<String>,
    total_sessions: i64,
    total_messages: i64,
    sessions: Vec<crate::types::ApiSession>,
    messages: Vec<ApiMessage>,
}

/// `context project`: every session in the project, newest first, and the
/// first `limit` messages, oldest first.
fn context_command_response(
    service: &QueryService,
    args: ContextArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let ContextCommand::Project(args) = args.command;
    let project = args
        .project
        .or_else(|| effective_project_scope(None, false));
    let sessions = service.sessions(
        project.as_deref(),
        source_filter,
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    )?;
    let messages = service.messages(
        &[],
        project.as_deref(),
        source_filter,
        MessageQueryOptions::new(Some(args.limit), 0, oldest_first()),
    )?;
    let response = ContextResponse {
        command: "context/project".to_string(),
        scope: "project".to_string(),
        source: source_filter.map(SourceFilter::as_str).map(str::to_string),
        project,
        total_sessions: sessions.total_sessions,
        total_messages: messages.total_messages,
        sessions: sessions.sessions,
        messages: messages.messages,
    };
    serialize(&response, pretty)
}

fn import_command_response(
    args: &ImportArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match &args.command {
        ImportCommand::Session(args) => match import_session_response(args, source_filter) {
            Ok(response) => serialize(&response, pretty),
            Err(failure) => {
                teleport_fail(rebrand_teleport_failure(failure, "import/session"), pretty)
            }
        },
        ImportCommand::Bundle(args) => import_bundle_response(args, pretty),
    }
}

fn share_command_response(
    args: &ShareArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match &args.command {
        ShareCommand::Session(args) => share_session_response(args, source_filter, pretty),
    }
}

fn import_bundle_response(args: &ImportBundleArgs, pretty: bool) -> Result<String> {
    let read_only =
        import_read_only_mode(args.read_only, args.apply, args.force).map_err(|failure| {
            anyhow::Error::new(
                CliFailure::from_teleport(
                    rebrand_teleport_failure(failure, "import/bundle"),
                    pretty,
                )
                .expect("serialize import mode failure"),
            )
        })?;
    let locator = match resolve_import_bundle_locator(args.locator.clone(), args.to.clone()) {
        Ok(locator) => locator,
        Err(failure) => {
            return teleport_fail(rebrand_teleport_failure(failure, "import/bundle"), pretty);
        }
    };

    if read_only {
        if locator == "-" {
            return teleport_fail(
                TeleportFailure::usage("import/bundle", "stdin bundle import requires --apply"),
                pretty,
            );
        }
        return match read_bundle(ReadOptions {
            locator,
            dry_run: args.dry_run,
            output_format: args.output_format.into(),
        }) {
            Ok(response) => {
                serialize_rebranded(&response, pretty, &[("teleport/read", "import/bundle")])
            }
            Err(failure) => {
                teleport_fail(rebrand_teleport_failure(failure, "import/bundle"), pretty)
            }
        };
    }

    if locator == "-" {
        return match apply_bundle(ApplyOptions {
            bundle_path: PathBuf::from("-"),
            project: args.project.clone(),
            dry_run: args.dry_run,
            force: args.force,
            skip_store_import: true,
        }) {
            Ok(response) => {
                serialize_rebranded(&response, pretty, &[("teleport/apply", "import/bundle")])
            }
            Err(failure) => {
                teleport_fail(rebrand_teleport_failure(failure, "import/bundle"), pretty)
            }
        };
    }

    match receive_bundle(ReceiveOptions {
        locator,
        dry_run: args.dry_run,
        project: args.project.clone(),
        force: args.force,
    }) {
        Ok(response) => serialize_rebranded(
            &response,
            pretty,
            &[
                ("teleport/receive", "import/bundle"),
                ("teleport/apply", "import/bundle/apply"),
            ],
        ),
        Err(failure) => teleport_fail(rebrand_teleport_failure(failure, "import/bundle"), pretty),
    }
}

fn share_session_response(
    args: &ShareSessionArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let session_id = match resolve_session_selector(
        args.selector.as_deref(),
        args.session.as_deref(),
        args.latest,
        "share/session",
    ) {
        Ok(session_id) => session_id,
        Err(failure) => return teleport_fail(failure, pretty),
    };
    let transport = args.via.unwrap_or(ShareTransportArg::Auto);
    if transport == ShareTransportArg::Http {
        if args.dry_run {
            return teleport_fail(
                TeleportFailure::usage(
                    "share/session",
                    "--dry-run is not supported with --via http",
                ),
                pretty,
            );
        }
        let service = QueryService::load()?;
        let project = args.project.clone().or_else(|| {
            if session_id.is_some() {
                None
            } else {
                effective_project_scope(None, false)
            }
        });
        return match serve_session(
            &service,
            ServeOptions {
                session_id,
                project,
                source_filter,
                bind: args.bind.clone().or(args.to.clone()),
                timeout_secs: args.timeout,
            },
        ) {
            Ok(()) => Ok(String::new()),
            Err(ServeError::BeforeStartup(failure)) => {
                teleport_fail(rebrand_teleport_failure(failure, "share/session"), pretty)
            }
            Err(ServeError::TimedOut) => Err(anyhow::Error::new(CliFailure::new(
                3,
                String::new(),
                "share session: timed out waiting for bundle download",
            ))),
        };
    }

    let to = match args.to.clone() {
        Some(to) => to,
        None => {
            return teleport_fail(
                TeleportFailure::usage(
                    "share/session",
                    "--to is required unless --via http is used",
                ),
                pretty,
            );
        }
    };
    let send_transport = match transport {
        ShareTransportArg::Auto => SendTransport::Auto,
        ShareTransportArg::Ssh => SendTransport::Ssh,
        ShareTransportArg::File => SendTransport::File,
        ShareTransportArg::Http => unreachable!("http handled above"),
    };
    let service = QueryService::load()?;
    let project = args.project.clone().or_else(|| {
        if session_id.is_some() {
            None
        } else {
            effective_project_scope(None, false)
        }
    });
    match send_session(
        &service,
        SendOptions {
            session_id,
            project,
            source_filter,
            to,
            transport: send_transport,
            dry_run: args.dry_run,
        },
    ) {
        Ok(response) => {
            let json =
                serialize_rebranded(&response, pretty, &[("teleport/send", "share/session")])?;
            if response.status == TeleportStatus::Partial {
                return Err(anyhow::Error::new(CliFailure::new(
                    3,
                    json,
                    "share session: remote mmr missing; bundle staged in remote inbox",
                )));
            }
            Ok(json)
        }
        Err(failure) => teleport_fail(rebrand_teleport_failure(failure, "share/session"), pretty),
    }
}

fn resolve_session_selector(
    positional: Option<&str>,
    flag: Option<&str>,
    latest: bool,
    command: &'static str,
) -> std::result::Result<Option<String>, TeleportFailure> {
    let supplied = [positional, flag].into_iter().flatten().count() + usize::from(latest);
    if supplied > 1 {
        return Err(TeleportFailure::usage(
            command,
            "pass only one of positional session, --session, or --latest",
        ));
    }
    let selector = positional.or(flag);
    Ok(match selector {
        Some("latest") | None => None,
        Some(session_id) => Some(session_id.to_string()),
    })
}

fn import_read_only_mode(
    read_only: bool,
    apply: bool,
    force: bool,
) -> std::result::Result<bool, TeleportFailure> {
    if read_only && apply {
        return Err(TeleportFailure::usage(
            "import/bundle",
            "pass either --read-only or --apply, not both",
        ));
    }
    if force && !apply {
        return Err(TeleportFailure::usage(
            "import/bundle",
            "--force requires --apply",
        ));
    }
    Ok(!apply)
}

fn resolve_import_bundle_locator(
    positional: Option<String>,
    to: Option<String>,
) -> std::result::Result<String, TeleportFailure> {
    match (positional, to) {
        (Some(_), Some(_)) => Err(TeleportFailure::usage(
            "import/bundle",
            "import bundle: only one locator is allowed; use either a positional locator or --to",
        )),
        (None, None) => Err(TeleportFailure::usage(
            "import/bundle",
            "import bundle: locator is required; pass a positional locator or --to",
        )),
        (Some(locator), None) | (None, Some(locator)) => Ok(locator),
    }
}

fn rebrand_teleport_failure(
    mut failure: TeleportFailure,
    command: &'static str,
) -> TeleportFailure {
    failure.command = command;
    failure.message = failure.message.replace("teleport", command);
    failure
}

fn serialize_rebranded<T: Serialize>(
    response: &T,
    pretty: bool,
    replacements: &[(&str, &str)],
) -> Result<String> {
    let mut value = serde_json::to_value(response)?;
    rebrand_command_values(&mut value, replacements);
    if pretty {
        Ok(serde_json::to_string_pretty(&value)?)
    } else {
        Ok(serde_json::to_string(&value)?)
    }
}

fn rebrand_command_values(value: &mut serde_json::Value, replacements: &[(&str, &str)]) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(command) = map.get_mut("command")
                && let Some(command_value) = command.as_str()
                && let Some((_, replacement)) =
                    replacements.iter().find(|(from, _)| *from == command_value)
            {
                *command = serde_json::Value::String((*replacement).to_string());
            }
            for child in map.values_mut() {
                rebrand_command_values(child, replacements);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rebrand_command_values(item, replacements);
            }
        }
        _ => {}
    }
}

fn teleport_fail(failure: TeleportFailure, pretty: bool) -> Result<String> {
    Err(anyhow::Error::new(CliFailure::from_teleport(
        failure, pretty,
    )?))
}

#[derive(Debug, Serialize, Deserialize)]
struct PeerTeleportPackResponse {
    command: String,
    status: String,
    bundle_id: String,
    bundle: TeleportBundleFile,
    remote_mmr_version: String,
}

#[derive(Debug, Serialize)]
struct ImportSessionResponse {
    command: String,
    status: TeleportStatus,
    transport: String,
    from: String,
    bundle_id: String,
    bundle_path: String,
    remote_mmr_version: String,
    read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    read: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apply: Option<serde_json::Value>,
}

fn import_session_response(
    pull: &ImportSessionArgs,
    source_filter: Option<SourceFilter>,
) -> std::result::Result<ImportSessionResponse, TeleportFailure> {
    if pull.session.is_some() && pull.latest {
        return Err(TeleportFailure::usage(
            "import/session",
            "pass either --session or --latest, not both",
        ));
    }
    let read_only = import_read_only_mode(pull.read_only, pull.apply, pull.force)
        .map_err(|failure| rebrand_teleport_failure(failure, "import/session"))?;
    let project_input = project_identity_input(pull.project.as_deref()).map_err(|error| {
        TeleportFailure::usage(
            "import/session",
            format!("resolve project identity: {error}"),
        )
    })?;
    let request = PeerTeleportPackRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        project: build_peer_project_identity(&project_input),
        source: source_filter,
        session_id: pull.session.clone(),
        latest: pull.latest || pull.session.as_deref() == Some("latest"),
    };
    let remote: PeerTeleportPackResponse = run_peer_json(
        &pull.from,
        &["mmr", "peer", "teleport-pack", "--request-json", "-"],
        Some(&request),
    )
    .map_err(|error| peer_error_to_teleport_failure("import/session", error))?;

    verify_artifact_hashes(&remote.bundle)
        .map_err(|error| TeleportFailure::runtime("import/session", error.to_string()))?;
    if remote.bundle_id != remote.bundle.manifest.bundle_id {
        return Err(TeleportFailure::runtime(
            "import/session",
            "peer bundle_id does not match manifest",
        ));
    }
    let bundle_id = remote.bundle_id.clone();
    let bundle_path = teleport_pull_cache_path(&bundle_id).map_err(|error| {
        TeleportFailure::runtime(
            "import/session",
            format!("resolve pull cache path: {error}"),
        )
    })?;
    write_bundle(&bundle_path, &remote.bundle)
        .map_err(|error| TeleportFailure::runtime("import/session", error.to_string()))?;

    let (read, apply, status) = if read_only {
        let read = read_bundle(ReadOptions {
            locator: bundle_path.display().to_string(),
            dry_run: false,
            output_format: TeleportOutputFormat::Json,
        })?;
        let status = read.status;
        (
            Some(
                serde_json::to_value(read)
                    .map_err(|error| {
                        TeleportFailure::runtime(
                            "import/session",
                            format!("serialize read response: {error}"),
                        )
                    })
                    .map(|mut value| {
                        rebrand_command_values(
                            &mut value,
                            &[("teleport/read", "import/session/read")],
                        );
                        value
                    })?,
            ),
            None,
            status,
        )
    } else {
        let apply = apply_bundle(ApplyOptions {
            bundle_path: bundle_path.clone(),
            project: pull.project.clone(),
            dry_run: false,
            force: pull.force,
            skip_store_import: true,
        })?;
        let status = apply.status;
        (
            None,
            Some(
                serde_json::to_value(apply)
                    .map_err(|error| {
                        TeleportFailure::runtime(
                            "import/session",
                            format!("serialize apply response: {error}"),
                        )
                    })
                    .map(|mut value| {
                        rebrand_command_values(
                            &mut value,
                            &[("teleport/apply", "import/session/apply")],
                        );
                        value
                    })?,
            ),
            status,
        )
    };

    Ok(ImportSessionResponse {
        command: "import/session".to_string(),
        status,
        transport: "ssh".to_string(),
        from: pull.from.clone(),
        bundle_id,
        bundle_path: bundle_path.display().to_string(),
        remote_mmr_version: remote.remote_mmr_version,
        read_only,
        read,
        apply,
    })
}

fn teleport_pull_cache_path(bundle_id: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("resolve HOME for teleport pull cache")?;
    Ok(home
        .join(".mmr")
        .join("teleport")
        .join("cache")
        .join(bundle_id)
        .join("bundle.mmr"))
}

fn peer_error_to_teleport_failure(
    command: &'static str,
    error: crate::peer::PeerCommandError,
) -> TeleportFailure {
    let exit_code = if error.error_kind == "peer_target_invalid" {
        2
    } else {
        3
    };
    let mut failure =
        TeleportFailure::runtime(command, error.message).with_error_kind(error.error_kind);
    failure.exit_code = exit_code;
    failure
}

fn peer_command_response(
    args: &PeerArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match &args.command {
        PeerCommand::Status(args) => {
            let _ = args.json;
            if let Some(host) = &args.host {
                let response: PeerStatusResponse = run_peer_json::<serde_json::Value, _>(
                    host,
                    &["mmr", "peer", "status", "--json"],
                    None,
                )
                .map_err(|error| peer_anyhow_error("peer/status", error, pretty))?;
                return serialize(&response, pretty);
            }
            serialize(&peer_status(), pretty)
        }
        PeerCommand::TeleportPack(args) => {
            serve_peer(&args.request_json, pretty, |service, request| {
                peer_teleport_pack_local_response(service, request, source_filter)
            })
        }
    }
}

/// Answer one `mmr peer <command> --request-json -` call on this machine.
fn serve_peer<Req, Resp, F>(locator: &str, pretty: bool, respond: F) -> Result<String>
where
    Req: for<'de> Deserialize<'de>,
    Resp: Serialize,
    F: FnOnce(&QueryService, Req) -> Result<Resp>,
{
    let request: Req = read_peer_request(locator)?;
    let service = QueryService::load()?;
    serialize(&respond(&service, request)?, pretty)
}

fn read_peer_request<T: for<'de> Deserialize<'de>>(locator: &str) -> Result<T> {
    if locator != "-" {
        bail!("peer request JSON must be read from stdin with --request-json -");
    }
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("read peer request JSON from stdin")?;
    serde_json::from_str(&input).context("parse peer request JSON")
}

fn peer_teleport_pack_local_response(
    service: &QueryService,
    request: PeerTeleportPackRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<PeerTeleportPackResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = request.source.or(cli_source_filter);
    let project = resolve_peer_project(service, &request.project, source)?;
    let session_id = match request.session_id.as_deref() {
        Some("latest") | None => None,
        Some(other) => Some(other.to_string()),
    };
    let pack = pack_session(
        service,
        PackOptions {
            session_id,
            project,
            source_filter: source,
            output_path: None,
            fidelity: TeleportFidelity::Native,
            dry_run: false,
        },
    )
    .map_err(|failure| anyhow::anyhow!(failure.message))?;
    let bundle_path = pack
        .bundle_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("teleport pack did not produce a bundle path"))?;
    let bundle = crate::teleport::load_bundle(Path::new(bundle_path))?;
    Ok(PeerTeleportPackResponse {
        command: "peer/teleport-pack".to_string(),
        status: "ok".to_string(),
        bundle_id: pack.bundle_id,
        bundle,
        remote_mmr_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

fn validate_peer_protocol(version: u32) -> Result<()> {
    if version != PEER_PROTOCOL_VERSION {
        bail!("unsupported peer protocol version {version}; expected {PEER_PROTOCOL_VERSION}");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct SkillInstallResponse {
    command: String,
    scope: String,
    path: String,
    replaced: bool,
    files: Vec<String>,
}

fn skill_command_response(args: &SkillArgs, pretty: bool) -> Result<String> {
    match &args.command {
        SkillCommand::Load => Ok(render_bundled_mmr_skill()),
        SkillCommand::Install(args) => serialize(&install_bundled_mmr_skill(args.local)?, pretty),
    }
}

fn render_bundled_mmr_skill() -> String {
    let mut output = String::from(
        "# mmr skill bundle\n\n\
This is the bundled `mmr` skill. Read `mmr/SKILL.md` first, then load referenced files only when needed.\n",
    );
    for file in BUNDLED_MMR_SKILL_FILES {
        output.push_str("\n---\n\n");
        output.push_str("## mmr/");
        output.push_str(file.relative_path);
        output.push_str("\n\n");
        output.push_str(file.contents.trim_end());
        output.push('\n');
    }
    output
}

fn install_bundled_mmr_skill(local: bool) -> Result<SkillInstallResponse> {
    let target = if local {
        std::env::current_dir()
            .context("current_dir")?
            .join(".agents")
            .join("skills")
            .join("mmr")
    } else {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        PathBuf::from(home)
            .join(".agents")
            .join("skills")
            .join("mmr")
    };
    let replaced = remove_existing_skill_target(&target)?;
    for file in BUNDLED_MMR_SKILL_FILES {
        let path = target.join(file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create skill directory {}", parent.display()))?;
        }
        fs::write(&path, file.contents)
            .with_context(|| format!("write skill file {}", path.display()))?;
    }

    Ok(SkillInstallResponse {
        command: "skill/install".to_string(),
        scope: if local { "local" } else { "user" }.to_string(),
        path: target.display().to_string(),
        replaced,
        files: BUNDLED_MMR_SKILL_FILES
            .iter()
            .map(|file| file.relative_path.to_string())
            .collect(),
    })
}

fn remove_existing_skill_target(target: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect existing skill target {}", target.display()));
        }
    };

    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(target)
            .with_context(|| format!("remove existing skill directory {}", target.display()))?;
    } else {
        fs::remove_file(target)
            .with_context(|| format!("remove existing skill file {}", target.display()))?;
    }
    Ok(true)
}

fn shell_quote(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Reject the not-by-default newest session (age 0) unless the caller opted in with
/// `--include-newest`. Out-of-range ages depend on the in-scope session count and are
/// enforced later in the service against the recency ranking.
fn validate_session_back(age: u32, include_newest: bool) -> Result<u32, SessionSelectionError> {
    if age == 0 && !include_newest {
        return Err(SessionSelectionError::AgeZeroNotSelectable);
    }
    Ok(age)
}

fn effective_project_scope(explicit_project: Option<String>, all: bool) -> Option<String> {
    select_project_scope(
        explicit_project,
        all,
        auto_discover_project_enabled(),
        auto_discovered_project_scope(),
    )
}

fn select_project_scope(
    explicit_project: Option<String>,
    all: bool,
    auto_discover_enabled: bool,
    discovered_project: Option<String>,
) -> Option<String> {
    explicit_project.or({
        if all || !auto_discover_enabled {
            None
        } else {
            discovered_project
        }
    })
}

fn auto_discovered_project_scope() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    discovered_project_scope_from_dir(&cwd)
}

fn discovered_project_scope_from_dir(path: &Path) -> Option<String> {
    resolve_project_from_dir(path)
        .ok()
        .map(|(codex_path, _)| codex_path)
}

fn auto_discover_project_enabled() -> bool {
    match std::env::var(ENV_AUTO_DISCOVER_PROJECT) {
        Ok(value) => match value.trim() {
            "0" => false,
            "" | "1" => true,
            _ => true,
        },
        Err(_) => true,
    }
}

/// Resolve current working directory to (codex_project_path, claude_project_name).
/// Codex uses the canonical path as-is; Claude uses path with '/' replaced by '-'
/// (e.g. /Users/mish/proj -> -Users-mish-proj).
fn resolve_project_from_cwd() -> Result<(String, String)> {
    let path = std::env::current_dir().context("current_dir")?;
    resolve_project_from_dir(&path)
}

fn resolve_project_from_dir(path: &Path) -> Result<(String, String)> {
    let path = path.canonicalize().context("canonicalize")?;
    let codex_path = path.to_string_lossy().into_owned();
    let claude_name = if codex_path == "/" {
        "-".to_string()
    } else {
        format!("-{}", codex_path.trim_start_matches('/').replace('/', "-"))
    };
    Ok((codex_path, claude_name))
}

fn current_dir_project() -> Result<String> {
    Ok(std::env::current_dir()?.to_string_lossy().into_owned())
}

fn path_basename(value: &str) -> Option<&str> {
    Path::new(value.trim())
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
}

fn project_identity_input(project: Option<&str>) -> Result<String> {
    match project {
        Some(project) => Ok(project.to_string()),
        None => current_dir_project(),
    }
}

fn build_peer_project_identity(project: &str) -> PeerProjectIdentity {
    let path = PathBuf::from(project);
    let canonical = if path.exists() {
        path.canonicalize()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| project.to_string())
    } else {
        project.to_string()
    };
    let display_name = Path::new(&canonical)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(canonical.as_str())
        .to_string();
    let git_root = git_root_for_path(Path::new(&canonical));
    let git_remotes = git_root
        .as_deref()
        .map(|root| git_remotes_for_path(Path::new(root)))
        .unwrap_or_default();
    let repo_fingerprint = if git_remotes.is_empty() {
        None
    } else {
        let mut remotes = git_remotes.clone();
        remotes.sort();
        Some(crate::teleport::hash_text(&remotes.join("\n")))
    };
    PeerProjectIdentity {
        local_path: canonical,
        display_name,
        git_root,
        git_remotes,
        repo_fingerprint,
    }
}

fn resolve_peer_project(
    service: &QueryService,
    identity: &PeerProjectIdentity,
    source_filter: Option<SourceFilter>,
) -> Result<Option<String>> {
    let projects = service.projects(
        source_filter,
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    );

    if !identity.git_remotes.is_empty() {
        let wanted = normalize_git_remotes(&identity.git_remotes);
        let mut matches = projects
            .projects
            .iter()
            .filter(|project| {
                let candidate_path = if project.original_path.is_empty() {
                    project.name.as_str()
                } else {
                    project.original_path.as_str()
                };
                if !Path::new(candidate_path).exists() {
                    return false;
                }
                let remotes = git_remotes_for_path(Path::new(candidate_path));
                !remotes.is_empty() && normalize_git_remotes(&remotes) == wanted
            })
            .map(|project| project.original_path.clone())
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        if matches.len() == 1 {
            return Ok(matches.pop());
        }
        if matches.len() > 1 {
            bail!(
                "multiple remote projects matched git remotes for {}; pass an explicit project",
                identity.display_name
            );
        }
    }

    let exact = projects
        .projects
        .iter()
        .find(|project| {
            project.name == identity.local_path || project.original_path == identity.local_path
        })
        .map(|project| {
            if project.original_path.is_empty() {
                project.name.clone()
            } else {
                project.original_path.clone()
            }
        });
    if exact.is_some() {
        return Ok(exact);
    }

    let mut display_matches = projects
        .projects
        .iter()
        .filter(|project| {
            project.name == identity.display_name
                || path_basename(&project.name) == Some(identity.display_name.as_str())
                || path_basename(&project.original_path) == Some(identity.display_name.as_str())
                || project
                    .aliases
                    .iter()
                    .any(|alias| alias == &identity.display_name)
        })
        .map(|project| {
            if project.original_path.is_empty() {
                project.name.clone()
            } else {
                project.original_path.clone()
            }
        })
        .collect::<Vec<_>>();
    display_matches.sort();
    display_matches.dedup();
    if display_matches.len() == 1 {
        return Ok(display_matches.pop());
    }
    if display_matches.len() > 1 {
        bail!(
            "multiple remote projects matched display name {}; pass an explicit project",
            identity.display_name
        );
    }

    Ok(Some(identity.local_path.clone()))
}

fn git_root_for_path(path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn git_remotes_for_path(path: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("config")
        .arg("--get-regexp")
        .arg("^remote\\..*\\.url$")
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut remotes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(str::to_string)
        .collect::<Vec<_>>();
    remotes.sort();
    remotes.dedup();
    remotes
}

fn normalize_git_remotes(remotes: &[String]) -> Vec<String> {
    let mut normalized = remotes
        .iter()
        .map(|remote| remote.trim().trim_end_matches(".git").to_ascii_lowercase())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

/// Resolve and serialize `recall N`: the session N sessions back from the newest in scope.
///
/// Recency-derived ages are unstable across time, so a paged result pins `next_command`
/// to the resolved session id rather than echoing the age; a session landing between
/// calls then cannot shift the window.
fn recall_response(
    service: &QueryService,
    args: RecallArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    // Fail fast on the age-0 rule (out-of-range needs the scope count and fails in the service).
    if let Err(error) = validate_session_back(args.n, args.include_newest) {
        return Err(anyhow::Error::new(session_selection_cli_failure(
            error, pretty,
        )?));
    }
    let project_scope = effective_project_scope(args.project, args.all);
    let options = MessageQueryOptions::new(Some(args.limit), args.offset, oldest_first());
    let outcome = service.messages_by_session_age(
        project_scope.as_deref(),
        args.all,
        source_filter,
        args.n,
        args.include_newest,
        options,
    )?;
    let mut response = match outcome {
        Ok(response) => response,
        Err(error) => {
            return Err(anyhow::Error::new(session_selection_cli_failure(
                error, pretty,
            )?));
        }
    };
    if response.next_page {
        let session_id = response
            .session_selection
            .as_ref()
            .and_then(|selection| selection.selected.first())
            .map(|selected| selected.session_id.as_str());
        response.next_command = Some(build_next_recall_read_command(
            source_filter,
            pretty,
            session_id,
            project_scope.as_deref(),
            args.all,
            args.limit,
            response.next_offset as usize,
        ));
    }
    serialize(&response, pretty)
}

/// The `read session` command for the next page of a local `recall`.
fn build_next_recall_read_command(
    source: Option<SourceFilter>,
    pretty: bool,
    session_id: Option<&str>,
    project: Option<&str>,
    all: bool,
    limit: usize,
    next_offset: usize,
) -> String {
    let mut parts = vec!["mmr".to_string()];
    if pretty {
        parts.push("--pretty".to_string());
    }
    if let Some(source) = source {
        parts.push(format!("--source {}", source.as_str()));
    }
    match session_id {
        Some(session_id) => {
            parts.push("read session".to_string());
            parts.push(shell_quote(session_id));
        }
        None => parts.push("read project".to_string()),
    }
    if let Some(project) = project {
        parts.push("--project".to_string());
        parts.push(shell_quote(project));
    }
    if all {
        parts.push("--all".to_string());
    }
    parts.push(format!("--limit {limit}"));
    parts.push(format!("--offset {next_offset}"));
    parts.join(" ")
}

/// Build the structured CLI failure (machine JSON on stdout, message on stderr, exit 2) for an
/// out-of-range / age-0 session-axis request, naming the relevant counts.
fn session_selection_cli_failure(error: SessionSelectionError, pretty: bool) -> Result<CliFailure> {
    let mut value = serde_json::json!({
        "command": "recall",
        "status": "failed",
        "error_kind": error.error_kind(),
        "message": error.to_string(),
    });
    match &error {
        SessionSelectionError::AgeZeroNotSelectable => {}
        SessionSelectionError::SessionBackOutOfRange {
            total_sessions_in_scope,
            max_selectable_age,
            requested,
        } => {
            value["total_sessions_in_scope"] = serde_json::json!(total_sessions_in_scope);
            value["max_selectable_age"] = serde_json::json!(max_selectable_age);
            value["requested_age"] = serde_json::json!(requested);
        }
    }
    let stdout = serialize(&value, pretty)?;
    Ok(CliFailure::new(2, stdout, error.to_string()))
}

fn peer_error_cli_failure(
    command: &str,
    error: crate::peer::PeerCommandError,
    pretty: bool,
) -> Result<CliFailure> {
    peer_cli_failure(
        command,
        error.error_kind,
        Some(error.host.as_str()),
        &error.message,
        pretty,
    )
}

fn peer_anyhow_error(
    command: &str,
    error: crate::peer::PeerCommandError,
    pretty: bool,
) -> anyhow::Error {
    match peer_error_cli_failure(command, error, pretty) {
        Ok(failure) => anyhow::Error::new(failure),
        Err(error) => error,
    }
}

fn peer_cli_failure(
    command: &str,
    error_kind: &str,
    host: Option<&str>,
    message: &str,
    pretty: bool,
) -> Result<CliFailure> {
    let mut value = serde_json::json!({
        "command": command,
        "status": "failed",
        "error_kind": error_kind,
        "message": message,
    });
    if let Some(host) = host {
        value["host"] = serde_json::json!(host);
    }
    let exit_code = if error_kind == "peer_target_invalid" || error_kind == "peer_protocol_error" {
        2
    } else {
        3
    };
    Ok(CliFailure::new(
        exit_code,
        serialize(&value, pretty)?,
        message,
    ))
}

fn serialize<T: Serialize>(value: &T, pretty: bool) -> Result<String> {
    if pretty {
        Ok(serde_json::to_string_pretty(value)?)
    } else {
        Ok(serde_json::to_string(value)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_summary_and_remember_do_not_parse() {
        assert!(Cli::try_parse_from(["mmr", "summary", "all"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "remember", "all"]).is_err());
    }

    #[test]
    fn recall_defaults_to_one_session_back() {
        let parsed = Cli::try_parse_from(["mmr", "recall"]).expect("recall should parse");
        let Commands::Recall(args) = parsed.command else {
            panic!("expected recall command");
        };
        assert_eq!(args.n, 1);
        assert!(!args.all);
    }

    #[test]
    fn recall_accepts_explicit_count_and_scope() {
        let parsed = Cli::try_parse_from(["mmr", "recall", "2", "--all"])
            .expect("recall 2 --all should parse");
        let Commands::Recall(args) = parsed.command else {
            panic!("expected recall command");
        };
        assert_eq!(args.n, 2);
        assert!(args.all);
    }

    #[test]
    fn read_session_parses() {
        let parsed =
            Cli::try_parse_from(["mmr", "read", "session", "sess-123"]).expect("read session");
        let Commands::Read(args) = parsed.command else {
            panic!("expected read command");
        };
        let ReadCommand::Session(session) = args.command else {
            panic!("expected read session command");
        };
        assert_eq!(session.session_id, "sess-123");
    }

    #[test]
    fn skill_commands_parse() {
        let load = Cli::try_parse_from(["mmr", "skill", "load"]).expect("skill load");
        let Commands::Skill(args) = load.command else {
            panic!("expected skill command");
        };
        assert!(matches!(args.command, SkillCommand::Load));

        let install =
            Cli::try_parse_from(["mmr", "skill", "install", "--local"]).expect("skill install");
        let Commands::Skill(args) = install.command else {
            panic!("expected skill command");
        };
        let SkillCommand::Install(install) = args.command else {
            panic!("expected skill install command");
        };
        assert!(install.local);
    }

    #[test]
    fn old_messages_and_prev_do_not_parse() {
        assert!(Cli::try_parse_from(["mmr", "messages", "--session", "a"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "prev"]).is_err());
    }

    #[test]
    fn validate_session_back_rejects_age_zero_without_include_newest() {
        assert_eq!(
            validate_session_back(0, false),
            Err(SessionSelectionError::AgeZeroNotSelectable)
        );
        assert_eq!(validate_session_back(0, true), Ok(0));
        assert_eq!(validate_session_back(1, false), Ok(1));
        // Out-of-range ages are a scope concern, not rejected by this validator.
        assert_eq!(validate_session_back(5, false), Ok(5));
    }

    #[test]
    fn select_project_scope_handles_explicit_all_and_failed_discovery_cases() {
        assert_eq!(
            select_project_scope(
                Some("/Users/test/explicit".to_string()),
                false,
                true,
                Some("/Users/test/discovered".to_string()),
            ),
            Some("/Users/test/explicit".to_string())
        );
        assert_eq!(
            select_project_scope(
                None,
                false,
                true,
                Some("/Users/test/discovered".to_string()),
            ),
            Some("/Users/test/discovered".to_string())
        );
        assert_eq!(select_project_scope(None, false, true, None), None);
        assert_eq!(
            select_project_scope(None, true, true, Some("/Users/test/discovered".to_string()),),
            None
        );
        assert_eq!(
            select_project_scope(
                None,
                false,
                false,
                Some("/Users/test/discovered".to_string()),
            ),
            None
        );
    }

    #[test]
    fn discovered_project_scope_from_dir_returns_none_for_missing_path() {
        let missing = std::env::temp_dir().join(format!("mmr-missing-{}", std::process::id()));
        assert_eq!(discovered_project_scope_from_dir(&missing), None);
    }

    #[test]
    fn remember_legacy_flags_do_not_parse() {
        assert!(Cli::try_parse_from(["mmr", "remember", "--mode", "all"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "remember", "--session-id", "sess-123"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "remember", "--continue-from", "abc"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "remember", "--follow-up", "next"]).is_err());
    }

    #[test]
    fn prompt_command_is_rejected() {
        assert!(Cli::try_parse_from(["mmr", "prompt", "fix bug", "--target", "codex"]).is_err());
    }

    #[test]
    fn merge_command_is_rejected() {
        assert!(Cli::try_parse_from(["mmr", "merge", "--from-session", "sess-a"]).is_err());
    }

    #[test]
    fn find_command_rejects_store_era_flags() {
        assert!(Cli::try_parse_from(["mmr", "find", "panic", "--all"]).is_ok());
        for flag in ["--role", "--event-type", "--format", "-C"] {
            assert!(
                Cli::try_parse_from(["mmr", "find", "panic", flag, "x"]).is_err(),
                "{flag}"
            );
        }
        assert!(Cli::try_parse_from(["mmr", "find", "panic", "-i"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "rg", "panic"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "search", "decision"]).is_err());
    }

    #[test]
    fn import_session_and_bundle_commands_parse() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "import",
            "session",
            "--from",
            "mini",
            "--session",
            "latest",
            "--project",
            "/tmp/project",
            "--read-only",
        ])
        .expect("import session should parse");
        let Commands::Import(args) = parsed.command else {
            panic!("expected import command");
        };
        let ImportCommand::Session(args) = args.command else {
            panic!("expected import session");
        };
        assert_eq!(args.project.as_deref(), Some("/tmp/project"));
        assert_eq!(args.from, "mini");
        assert!(args.read_only);

        let parsed = Cli::try_parse_from([
            "mmr",
            "import",
            "bundle",
            "./handoff.mmr",
            "--apply",
            "--force",
        ])
        .expect("import bundle should parse");
        let Commands::Import(args) = parsed.command else {
            panic!("expected import command");
        };
        let ImportCommand::Bundle(args) = args.command else {
            panic!("expected import bundle");
        };
        assert_eq!(args.locator.as_deref(), Some("./handoff.mmr"));
        assert!(args.apply);
        assert!(args.force);
    }

    #[test]
    fn store_commands_are_rejected() {
        for argv in [
            &["mmr", "init"][..],
            &["mmr", "ingest", "events", "--project", "/tmp/project"],
            &["mmr", "note", "decision"],
            &["mmr", "retrieve", "retry"],
            &["mmr", "status"],
            &["mmr", "sync", "--dry-run"],
            &["mmr", "redact", "scan"],
            &["mmr", "__db-info"],
            &["mmr", "read", "session", "s1", "--format", "tree"],
            &["mmr", "read", "project", "--output-dir", "/tmp/tree"],
        ] {
            let error = Cli::try_parse_from(argv).expect_err(&argv.join(" "));
            assert_eq!(error.exit_code(), 2, "{}", argv.join(" "));
        }
    }
}
