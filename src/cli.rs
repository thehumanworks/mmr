mod retrieval;

use retrieval::{find_output, retrieve_output};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::agent::{ai, compact};
use crate::capture::{
    ClaudeAdapter, CodexAdapter, CursorAdapter, Reconciler, SourceAdapter, SourceDiscoveryRoot,
};
use crate::config::{
    self, DEFAULT_SUMMARISER_MODEL, summarize_api_key_configured, summarize_endpoint_for_status,
};
use crate::messages::service::{MessageQueryOptions, QueryService, SessionSelectionError};
use crate::peer::{
    PEER_PROTOCOL_VERSION, PeerProjectIdentity, PeerStatusResponse, PeerTeleportPackRequest,
    peer_status, run_peer_json,
};
use crate::redaction::{
    PiiCoverage, PiiCoverageStatus, RedactionFinding, RedactionOutcome, scan_text,
};
use crate::store::{
    DEFAULT_REDACTION_POLICY_ID, EventRecord, LATEST_SCHEMA_VERSION, NewEvent, NewRedactionSpan,
    ProjectRecord, Store, content_hash, default_db_path,
};
use crate::sync::{
    HydrationReport, RemoteSummary, SyncReport, hydrate_project, remote_for_operations,
    remote_for_status, safe_projection_blocker, sync_project,
};
use crate::teleport::{
    ApplyOptions, PackOptions, ReadOptions, ReceiveOptions, SendOptions, SendTransport, ServeError,
    ServeOptions, TeleportBundleFile, TeleportFailure, TeleportFidelity, TeleportOutputFormat,
    TeleportStatus, apply_bundle, pack_session, read_bundle, receive_bundle, send_session,
    serve_session, verify_artifact_hashes, write_bundle,
};
use crate::types::{
    ApiMessage, ApiMessagesResponse, CompactResponse, RememberRequest, RememberResponse, SortBy,
    SortOptions, SortOrder, SourceFilter,
};

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
const ENV_DEFAULT_SOURCE: &str = "MMR_DEFAULT_SOURCE";
const ENV_COMPACT_MODEL: &str = "MMR_COMPACT_MODEL";

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
    about = "Browse AI conversation history from Claude, Codex, Cursor, Grok, and Pi",
    after_help = "Examples:\n  mmr init\n  mmr status --pretty\n  mmr list projects --pretty\n  mmr read session <session-id> --pretty\n  mmr read project --format tree --output-dir /tmp/mmr-tree\n  mmr share session latest --to user@host\n  mmr import session --from mini --session latest --project /path/to/project --read-only\n  mmr --source codex ingest events --project /path/to/project\n  mmr find \"migration append-only\" --format line\n  mmr summarize project --project /path/to/project\n  mmr compact project --project /path/to/project --query \"current task\"\n  mmr skill load\n  mmr skill install --local\n  mmr mcp --transport stdio\n  mmr mcp --transport http\n  mmr sync --pretty\n\nOutput:\n  Commands emit machine-readable JSON on stdout unless an explicit stream format such as --format line is selected. Use --pretty for indented JSON. `mmr mcp --transport stdio` reserves stdout for MCP protocol frames."
)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Filter by source: claude, codex, cursor, grok, pi (omit to use MMR_DEFAULT_SOURCE or all)
    #[arg(long, global = true, value_enum)]
    pub source: Option<SourceFilter>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Set up or repair the local mmr store for the current project
    Init(InitArgs),
    /// List known projects or sessions
    List(ListArgs),
    /// Search normalized events and learned memory
    Find(SearchTextArgs),
    /// Search normalized matches, then read bounded provider transcript windows
    Retrieve(RetrieveArgs),
    /// Retrieve a previous stable session for immediate continuity
    Recall(RecallArgs),
    /// Read raw session, project, or source history
    Read(ReadArgs),
    /// Produce scoped context for future agents
    Context(ContextArgs),
    /// Run a stateless summary over scoped history
    Summarize(SummarizeArgs),
    /// Compact scoped history with Morph Compact without rewriting surviving lines
    Compact(CompactArgs),
    /// Load or install the bundled mmr agent skill
    Skill(SkillArgs),
    /// Import session or bundle material into this machine
    Import(ImportArgs),
    /// Ingest normalized source events into the local memory store
    Ingest(IngestArgs),
    /// Share one selected session from this machine
    Share(ShareArgs),
    /// Add a human-authored note to the local memory store
    Note {
        /// Note text. Omit to read multiline text from stdin.
        #[arg(value_name = "TEXT", trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Inspect and apply local redaction policy before sync
    Redact(RedactArgs),
    /// Safely reconcile the linked project with the default mmr-store remote
    Sync(SyncArgs),
    /// Inspect local project, redaction, and sync state
    Status(StatusArgs),
    /// Run mmr as a Model Context Protocol server
    Mcp(crate::mcp::McpArgs),
    /// Inspect the local mmr database path and schema version
    #[command(name = "__db-info", hide = true)]
    DbInfo {
        /// Link this project path before reporting store info
        #[arg(long)]
        project: Option<PathBuf>,
        /// Insert and read one synthetic event for CLI smoke testing
        #[arg(long)]
        smoke_event: bool,
    },
    /// Implementation-facing SSH peer protocol
    #[command(hide = true)]
    Peer(PeerArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Link the project and report suggested imports without ingesting source history
    #[arg(long)]
    link_only: bool,
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
    /// Output format
    #[arg(long, value_enum, default_value = "json")]
    format: ReadFormatArg,
    /// Output directory for --format tree
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Maximum number of messages to return for JSON output
    #[arg(long)]
    limit: Option<usize>,
    /// Number of sorted messages to skip for JSON output
    #[arg(long, default_value_t = 0)]
    offset: usize,
}

#[derive(Args, Debug)]
pub struct ReadProjectArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long)]
    project: Option<String>,
    /// Output format
    #[arg(long, value_enum, default_value = "json")]
    format: ReadFormatArg,
    /// Output directory for --format tree
    #[arg(long)]
    output_dir: Option<PathBuf>,
    /// Maximum number of messages to return for JSON output
    #[arg(long)]
    limit: Option<usize>,
    /// Number of sorted messages to skip for JSON output
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
pub struct SummarizeArgs {
    #[command(subcommand)]
    command: SummarizeCommand,
}

#[derive(Subcommand, Debug)]
pub enum SummarizeCommand {
    /// Run a stateless summary over project-scoped history
    Project(SummarizeProjectArgs),
    /// Run a stateless summary over one explicit session
    Session(SummarizeSessionArgs),
}

#[derive(Args, Debug)]
pub struct SummarizeProjectArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long, short = 'p')]
    project: Option<String>,
    /// Maximum number of messages to include (newest-first window; same as `read project`)
    #[arg(long)]
    limit: Option<usize>,
    /// Number of newest-ranked messages to skip before applying `--limit`
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[command(flatten)]
    runner: SummaryRunnerArgs,
}

#[derive(Args, Debug)]
pub struct SummarizeSessionArgs {
    /// Session ID to summarize
    session_id: String,
    /// Project name or path (optional; without it the session is searched globally)
    #[arg(long, short = 'p')]
    project: Option<String>,
    /// Maximum number of messages to include (newest-first window; same as `read session`)
    #[arg(long)]
    limit: Option<usize>,
    /// Number of newest-ranked messages to skip before applying `--limit`
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[command(flatten)]
    runner: SummaryRunnerArgs,
}

#[derive(Args, Debug)]
pub struct CompactArgs {
    #[command(subcommand)]
    command: CompactCommand,
}

#[derive(Subcommand, Debug)]
pub enum CompactCommand {
    /// Compact project-scoped history with Morph Compact
    Project(CompactProjectArgs),
    /// Compact one explicit session with Morph Compact
    Session(CompactSessionArgs),
}

#[derive(Args, Debug)]
pub struct CompactProjectArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long, short = 'p')]
    project: Option<String>,
    #[command(flatten)]
    runner: CompactRunnerArgs,
}

#[derive(Args, Debug)]
pub struct CompactSessionArgs {
    /// Session ID to compact
    session_id: String,
    /// Project name or path (optional; without it the session is searched globally)
    #[arg(long, short = 'p')]
    project: Option<String>,
    #[command(flatten)]
    runner: CompactRunnerArgs,
}

#[derive(Args, Debug, Clone)]
pub struct CompactRunnerArgs {
    /// Focus query for relevance-based pruning
    #[arg(long)]
    query: Option<String>,
    /// Fraction of input to keep, e.g. 0.3 aggressive or 0.7 light
    #[arg(long = "compression-ratio")]
    compression_ratio: Option<f32>,
    /// Keep the last N messages uncompressed
    #[arg(long = "preserve-recent")]
    preserve_recent: Option<u32>,
    /// Omit compacted_line_ranges from Morph's response
    #[arg(long = "no-line-ranges")]
    no_line_ranges: bool,
    /// Omit '(filtered N lines)' markers from compacted output
    #[arg(long = "no-markers")]
    no_markers: bool,
    /// Output format for compact results
    #[arg(
        short = 'O',
        long = "output-format",
        value_enum,
        default_value = "json"
    )]
    output_format: CompactOutputFormatArg,
    /// Morph compact model to use (overrides MMR_COMPACT_MODEL)
    #[arg(long)]
    model: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct SummaryRunnerArgs {
    /// Override the output format and rules portion of the system instructions
    #[arg(long)]
    instructions: Option<String>,
    /// Output format for summary results
    #[arg(short = 'O', long = "output-format", value_enum, default_value = "md")]
    output_format: RememberOutputFormatArg,
    /// Model to use (overrides config and MMR_SUMMARISER_MODEL)
    #[arg(long)]
    model: Option<String>,
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
pub struct RedactArgs {
    #[command(subcommand)]
    command: RedactCommand,
}

#[derive(Subcommand, Debug)]
pub enum RedactCommand {
    /// Scan linked project events and persist redaction runs
    Scan {
        /// Project path (omit to use current directory)
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Explain the latest redaction run for an event
    Explain {
        /// Event ID to inspect
        event_id: String,
    },
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Show what would sync, without contacting a remote
    #[arg(long)]
    dry_run: bool,
    /// Project path (omit to use current directory)
    #[arg(long)]
    project: Option<PathBuf>,
}

#[derive(Args, Debug)]
#[command(
    after_help = "Status JSON includes store.db_path, store.schema_version, store.expected_schema_version, store.schema_status, remote state, project link state, and diagnostics for sources, privacy filtering, schema, sync, and continuity brief provider setup."
)]
pub struct StatusArgs {
    /// Project path (omit to use current directory)
    #[arg(long)]
    project: Option<PathBuf>,
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
pub enum RememberOutputFormatArg {
    Json,
    #[default]
    Md,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
pub enum CompactOutputFormatArg {
    #[default]
    Json,
    Md,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
pub enum ReadFormatArg {
    #[default]
    Json,
    Tree,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
pub enum FindFormatArg {
    #[default]
    Json,
    Line,
}

#[derive(Args, Debug)]
pub struct SearchTextArgs {
    /// Literal query or pattern to find
    query: String,
    /// Project path (omit to use current directory)
    #[arg(long)]
    project: Option<PathBuf>,
    /// Source session id to search
    #[arg(long)]
    session: Option<String>,
    /// Event role filter
    #[arg(long)]
    role: Option<String>,
    /// Event type filter
    #[arg(long = "event-type")]
    event_type: Option<String>,
    /// Case-insensitive literal matching
    #[arg(short = 'i', long)]
    ignore_case: bool,
    /// Context lines before and after each match
    #[arg(short = 'C', long, default_value_t = 0)]
    context: usize,
    /// Output format
    #[arg(long, value_enum, default_value = "json")]
    format: FindFormatArg,
}

#[derive(Args, Debug)]
pub struct RetrieveArgs {
    /// Literal query or pattern to find
    query: String,
    /// Project path (omit to use current directory)
    #[arg(long)]
    project: Option<PathBuf>,
    /// Search every local project discovered from provider transcripts
    #[arg(long = "all-projects")]
    all_projects: bool,
    /// Search every source and ignore MMR_DEFAULT_SOURCE
    #[arg(long = "all-sources")]
    all_sources: bool,
    /// Include retrieve execution metadata such as searched projects
    #[arg(long)]
    debug: bool,
    /// Include bounded provider message windows in selected sessions
    #[arg(long = "full-message-history")]
    full_message_history: bool,
    /// Source session id to search
    #[arg(long)]
    session: Option<String>,
    /// Event role filter
    #[arg(long)]
    role: Option<String>,
    /// Event type filter
    #[arg(long = "event-type")]
    event_type: Option<String>,
    /// Case-insensitive literal matching
    #[arg(short = 'i', long)]
    ignore_case: bool,
    /// Context lines before and after each match
    #[arg(short = 'C', long, default_value_t = 0)]
    context: usize,
    /// Maximum number of matched sessions to select
    #[arg(long = "max-sessions", default_value_t = 3)]
    max_sessions: usize,
    /// Provider messages before each matched anchor
    #[arg(long = "before-messages", default_value_t = 3)]
    before_messages: usize,
    /// Provider messages after each matched anchor
    #[arg(long = "after-messages", default_value_t = 12)]
    after_messages: usize,
    /// Maximum provider messages returned for each selected session before pagination
    #[arg(long = "max-messages-per-session", default_value_t = 24)]
    max_messages_per_session: usize,
    /// Flattened message-page limit
    #[arg(long)]
    limit: Option<usize>,
    /// Provider message offset within each selected session window
    #[arg(long, default_value_t = 0)]
    offset: usize,
    /// Frozen session identity from a previous retrieve next_command
    #[arg(long = "pinned-session")]
    pinned_sessions: Vec<String>,
}

#[derive(Args, Debug)]
pub struct IngestArgs {
    #[command(subcommand)]
    command: IngestCommand,
}

#[derive(Subcommand, Debug)]
pub enum IngestCommand {
    /// Ingest source events into the normalized local memory store
    Events(IngestEventsArgs),
}

#[derive(Args, Debug)]
pub struct IngestEventsArgs {
    /// Project path to link/ingest into
    #[arg(long)]
    project: PathBuf,
    /// Source root (defaults to $HOME/.codex, $HOME/.claude, or $HOME/.cursor based on --source)
    #[arg(long = "source-root")]
    source_root: Option<PathBuf>,
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
    let source_filter = effective_source(cli.source);
    let pretty = cli.pretty;
    let load_service = || QueryService::load_filtered(source_filter);
    match cli.command {
        Commands::Mcp(args) => crate::mcp::run_mcp(&args, pretty).await,
        Commands::Note { text } => serialize(&note_response(text)?, pretty),
        Commands::Find(args) => find_output(&args, source_filter, pretty),
        Commands::Skill(args) => skill_command_response(&args, pretty),
        Commands::Import(args) => import_command_response(&args, source_filter, pretty),
        Commands::Ingest(args) => ingest_command_response(&args, source_filter, pretty),
        Commands::Share(args) => share_command_response(&args, source_filter, pretty),
        Commands::Init(args) => serialize(&init_response(&args, source_filter)?, pretty),
        Commands::Redact(args) => serialize(&redact_response(&args, source_filter)?, pretty),
        Commands::Sync(args) => serialize(&sync_response(&args, source_filter)?, pretty),
        Commands::Status(args) => serialize(&status_response(&args, source_filter)?, pretty),
        Commands::DbInfo {
            project,
            smoke_event,
        } => serialize(&db_info_response(project, smoke_event)?, pretty),
        Commands::Peer(args) => peer_command_response(&args, source_filter, pretty),
        Commands::List(args) => {
            list_command_response(&load_service()?, args, source_filter, pretty)
        }
        Commands::Retrieve(args) => {
            // `--all-sources` without an explicit `--source` ignores MMR_DEFAULT_SOURCE.
            let load_filter = if args.all_sources && cli.source.is_none() {
                None
            } else {
                source_filter
            };
            let service = QueryService::load_filtered(load_filter)?;
            retrieve_output(&args, &service, cli.source, source_filter, pretty)
        }
        Commands::Recall(args) => {
            recall_response(&load_service()?, args, cli.source, source_filter, pretty)
        }
        Commands::Read(args) => {
            read_command_response(&load_service()?, args, source_filter, pretty)
        }
        Commands::Context(args) => {
            context_command_response(&load_service()?, args, source_filter, pretty)
        }
        Commands::Summarize(args) => {
            let service = load_service()?;
            summarize_command_response(&service, args, source_filter, pretty).await
        }
        Commands::Compact(args) => {
            let service = load_service()?;
            compact_command_response(&service, args, source_filter, pretty).await
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
    if args.format == ReadFormatArg::Tree {
        let store = Store::open_default()?;
        let events = store
            .events_for_source_session(&args.session_id, source_filter.map(SourceFilter::as_str))?;
        let response = export_tree_events_response(events, args.output_dir, "session")?;
        return serialize(&response, pretty);
    }

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
    if args.format == ReadFormatArg::Tree {
        let response = export_tree_project_response(args.project, args.output_dir, source_filter)?;
        return serialize(&response, pretty);
    }

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

/// The messages that one `summarize` or `compact` target covers.
#[derive(Clone, Copy)]
enum MessageScope<'a> {
    Project(&'a str),
    Session {
        session_id: &'a str,
        project: Option<&'a str>,
    },
}

impl MessageScope<'_> {
    /// Every local message in scope, oldest first.
    fn local_messages(
        self,
        service: &QueryService,
        source_filter: Option<SourceFilter>,
    ) -> Result<Vec<ApiMessage>> {
        let unpaged = MessageQueryOptions::new(None, 0, oldest_first());
        let response = match self {
            Self::Project(project) => {
                service.messages(&[], Some(project), source_filter, unpaged)?
            }
            Self::Session {
                session_id,
                project,
            } => service.messages(&[session_id.to_string()], project, source_filter, unpaged)?,
        };
        Ok(response.messages)
    }
}

fn paginate<T>(items: Vec<T>, limit: Option<usize>, offset: usize) -> Vec<T> {
    let iter = items.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

fn sort_api_messages_chronological(messages: &mut [ApiMessage]) {
    messages.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.project_name.cmp(&b.project_name))
            .then_with(|| a.role.cmp(&b.role))
    });
}

fn oldest_first() -> SortOptions {
    SortOptions::new(SortBy::Timestamp, SortOrder::Asc)
}

/// Keep one page counted back from the newest message, returned oldest first.
/// With no `--limit` and no `--offset`, keep every message in its current order.
fn newest_page(
    mut messages: Vec<ApiMessage>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<ApiMessage> {
    if limit.is_none() && offset == 0 {
        return messages;
    }
    sort_api_messages_chronological(&mut messages);
    messages.reverse();
    let mut page = paginate(messages, limit, offset);
    page.reverse();
    page
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

async fn summarize_command_response(
    service: &QueryService,
    args: SummarizeArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        SummarizeCommand::Project(args) => {
            let project = args
                .project
                .unwrap_or(current_dir_project().context("could not resolve current directory")?);
            if args.limit.is_none() && args.offset == 0 {
                let model = effective_summariser_model(args.runner.model.as_deref());
                let response = ai::remember(
                    service,
                    RememberRequest {
                        project: project.as_str(),
                        source: source_filter,
                        instructions: args.runner.instructions.as_deref(),
                        model: &model,
                    },
                )
                .await?;
                return format_remember_response(&response, args.runner.output_format, pretty);
            }
            let scope = MessageScope::Project(project.as_str());
            let messages = scope.local_messages(service, source_filter)?;
            let messages = newest_page(messages, args.limit, args.offset);
            summarize_transcript(&args.runner, &messages, pretty).await
        }
        SummarizeCommand::Session(args) => {
            let scope = MessageScope::Session {
                session_id: &args.session_id,
                project: args.project.as_deref(),
            };
            let messages = scope.local_messages(service, source_filter)?;
            let messages = newest_page(messages, args.limit, args.offset);
            summarize_transcript(&args.runner, &messages, pretty).await
        }
    }
}

async fn summarize_transcript(
    runner: &SummaryRunnerArgs,
    messages: &[ApiMessage],
    pretty: bool,
) -> Result<String> {
    let model = effective_summariser_model(runner.model.as_deref());
    let formatted = format_messages_as_transcript_input(messages);
    let response =
        ai::summarize_formatted_messages(runner.instructions.as_deref(), &model, &formatted)
            .await?;
    format_remember_response(&response, runner.output_format, pretty)
}

async fn compact_command_response(
    service: &QueryService,
    args: CompactArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let (messages, runner) = match args.command {
        CompactCommand::Project(args) => {
            let project = args
                .project
                .unwrap_or(current_dir_project().context("could not resolve current directory")?);
            let scope = MessageScope::Project(project.as_str());
            (scope.local_messages(service, source_filter)?, args.runner)
        }
        CompactCommand::Session(args) => {
            let scope = MessageScope::Session {
                session_id: &args.session_id,
                project: args.project.as_deref(),
            };
            (scope.local_messages(service, source_filter)?, args.runner)
        }
    };
    let formatted = format_messages_as_transcript_input(&messages);
    let response = compact_formatted_messages(&runner, &formatted).await?;
    format_compact_response(&response, runner.output_format, pretty)
}

async fn compact_formatted_messages(
    args: &CompactRunnerArgs,
    formatted_messages: &str,
) -> Result<CompactResponse> {
    validate_compression_ratio(args.compression_ratio)?;
    let model = effective_compact_model(args.model.as_deref());
    let client = compact::MorphCompactClient::from_env()?;
    let mut request = compact::CompactRequest::new(formatted_messages, &model);
    request.query = args.query.as_deref();
    request.compression_ratio = args.compression_ratio;
    request.preserve_recent = args.preserve_recent;
    request.include_line_ranges = args.no_line_ranges.then_some(false);
    request.include_markers = args.no_markers.then_some(false);

    let result = client.compact(request).await?;
    Ok(CompactResponse::new(
        result.model,
        result.id,
        result.output,
        result.messages,
        result.usage,
    ))
}

fn validate_compression_ratio(ratio: Option<f32>) -> Result<()> {
    if let Some(ratio) = ratio
        && !(0.05..=1.0).contains(&ratio)
    {
        bail!("--compression-ratio must be between 0.05 and 1.0");
    }
    Ok(())
}

fn format_messages_as_transcript_input(messages: &[ApiMessage]) -> String {
    let mut output = String::new();
    let mut current_session = String::new();
    for message in messages {
        if message.session_id != current_session {
            current_session = message.session_id.clone();
            output.push_str(&format!(
                "\n## Session {} ({}, {})\n",
                message.session_id, message.source, message.project_name
            ));
        }
        output.push_str(&format!(
            "- [{}] {}: {}\n",
            message.timestamp, message.role, message.content
        ));
    }
    output
}

fn ingest_command_response(
    args: &IngestArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match &args.command {
        IngestCommand::Events(args) => {
            serialize(&ingest_events_response(args, source_filter)?, pretty)
        }
    }
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

#[derive(Debug, Serialize)]
struct NoteResponse {
    project_id: String,
    event_id: String,
    source: String,
    citation: String,
}

#[derive(Debug, Serialize)]
struct LinkResponse {
    command: String,
    already_linked: bool,
    store: StoreStatus,
    project: ProjectStatus,
    remote: RemoteSummary,
    hydration: HydrationReport,
    imports: Vec<SourceImportStatus>,
    rebuilt_search_documents: usize,
    sync: SyncReport,
    status: StatusProjectSnapshot,
    suggested_import_commands: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    command: String,
    store: StatusStoreDiagnostics,
    project: Option<ProjectStatus>,
    remote: RemoteSummary,
    status: StatusProjectSnapshot,
    diagnostics: StatusDiagnostics,
}

#[derive(Debug, Serialize)]
struct StoreStatus {
    schema_version: i64,
}

#[derive(Debug, Serialize)]
struct StatusStoreDiagnostics {
    db_path: String,
    exists: bool,
    existed_before_command: bool,
    schema_version: i64,
    expected_schema_version: i64,
    schema_status: String,
}

fn note_response(text: Vec<String>) -> Result<NoteResponse> {
    let mut store = Store::open_default()?;
    let cwd = std::env::current_dir().context("current_dir")?;
    let project = store.project_by_path(&cwd)?.ok_or_else(|| {
        anyhow::anyhow!("current project is not linked; run `mmr init` before adding notes")
    })?;
    let content = read_note_text(text)?;
    let timestamp = now_rfc3339()?;
    let note_identity = content_hash(&format!("{timestamp}:{content}"));
    let source_event_id = format!("note:{note_identity}");
    let event = NewEvent::new(
        "note",
        "notes",
        "note",
        "user",
        timestamp.clone(),
        content,
        "note-v1",
    )
    .with_source_event_id(source_event_id)
    .with_raw_local_ref(format!("note://notes/{note_identity}"));
    let (event, search_document) = store.insert_event_with_search_document(&project.id, &event)?;

    Ok(NoteResponse {
        project_id: project.id,
        event_id: event.id,
        source: event.source,
        citation: search_document.citation,
    })
}

fn read_note_text(text: Vec<String>) -> Result<String> {
    let note = if text.is_empty() {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .context("read note from stdin")?;
        buffer
    } else {
        text.join(" ")
    };
    let note = note.trim_matches(['\n', '\r']).trim().to_string();
    if note.is_empty() {
        bail!("note text is empty");
    }
    Ok(note)
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

#[derive(Debug, Serialize)]
struct ProjectStatus {
    id: String,
    display_name: String,
    path_hash: String,
}

#[derive(Debug, Serialize)]
struct SourceImportStatus {
    source: String,
    status: String,
    discovered_sessions: usize,
    imported_events: usize,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StatusProjectSnapshot {
    linked: bool,
    sync_status: String,
    events_total: usize,
    source_counts: BTreeMap<String, usize>,
    sync_status_counts: BTreeMap<String, usize>,
    redaction: StatusRedactionSnapshot,
    sync: StatusSyncSnapshot,
}

#[derive(Debug, Serialize)]
struct StatusRedactionSnapshot {
    policy_id: String,
    redacted_or_synced: usize,
    blocked: usize,
    pending: usize,
}

#[derive(Debug, Serialize)]
struct StatusSyncSnapshot {
    remote_events: usize,
    local_manifests: usize,
    latest_manifest_id: Option<String>,
    blocked_events: usize,
    unsynced_events: usize,
}

#[derive(Debug, Serialize)]
struct StatusDiagnostics {
    schema: StatusSchemaDiagnostic,
    remote: StatusRemoteDiagnostic,
    sources: Vec<StatusSourceDiagnostic>,
    privacy_filter: StatusPrivacyDiagnostic,
    summary_runner: StatusSummaryRunnerDiagnostic,
    actions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StatusSchemaDiagnostic {
    status: String,
    current_version: i64,
    expected_version: i64,
    action: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusRemoteDiagnostic {
    status: String,
    descriptor: String,
    backend: String,
    available: bool,
    auth_status: String,
    action: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusSourceDiagnostic {
    source: String,
    status: String,
    event_count: usize,
    source_root: Option<String>,
    action: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusPrivacyDiagnostic {
    status: PiiCoverageStatus,
    detector: String,
    reason: String,
    action: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusSummaryRunnerDiagnostic {
    backend: String,
    status: String,
    endpoint: String,
    model: String,
    config_file: String,
    api_key_env: Vec<String>,
    action: Option<String>,
}

fn init_response(args: &InitArgs, source_filter: Option<SourceFilter>) -> Result<LinkResponse> {
    let mut store = Store::open_default()?;
    let info = store.info()?;
    let cwd = std::env::current_dir().context("current_dir")?;
    let already_linked = store.project_by_path(&cwd)?.is_some();
    let project = store.ensure_project_link(&cwd)?;
    let remote = remote_for_status()?;
    let remote_auth_ok = remote.summary().auth_status == "ok";
    let hydration = if remote_auth_ok {
        hydrate_project(&mut store, &project, &remote)?
    } else {
        HydrationReport {
            remote_events: 0,
            inserted_events: 0,
            existing_events: 0,
            remote_learned_memory: 0,
            inserted_learned_memory: 0,
            existing_learned_memory: 0,
        }
    };
    let imports = if args.link_only {
        Vec::new()
    } else {
        reconcile_default_sources(&mut store, &project, source_filter)?
    };
    let rebuilt_search_documents = if args.link_only {
        0
    } else {
        rebuild_search_documents(&store, &project, source_filter)?
    };
    let sync = if args.link_only {
        remote_pending_sync_report(&store, &project, &remote, source_filter)?
    } else if remote_auth_ok {
        sync_project(
            &mut store,
            &project,
            &remote,
            source_filter.map(SourceFilter::as_str),
        )?
    } else {
        remote_pending_sync_report(&store, &project, &remote, source_filter)?
    };
    let status = status_snapshot(&store, Some(&project), Some(&remote))?;
    Ok(LinkResponse {
        command: "init".to_string(),
        already_linked,
        store: StoreStatus {
            schema_version: info.schema_version,
        },
        project: project_status(&project),
        remote: sync.remote.clone(),
        hydration,
        imports,
        rebuilt_search_documents,
        sync,
        status,
        suggested_import_commands: suggested_import_commands(source_filter),
    })
}

fn suggested_import_commands(source_filter: Option<SourceFilter>) -> Vec<String> {
    match source_filter {
        Some(source) => vec![format!("mmr --source {} ingest events", source.as_str())],
        None => ["codex", "claude", "cursor"]
            .iter()
            .map(|source| format!("mmr --source {source} ingest events"))
            .collect(),
    }
}

fn remote_pending_sync_report(
    store: &Store,
    project: &ProjectRecord,
    remote: &crate::sync::FakeGithubRemote,
    source_filter: Option<SourceFilter>,
) -> Result<SyncReport> {
    let events =
        store.events_for_project(&project.id, source_filter.map(SourceFilter::as_str), None)?;
    Ok(SyncReport {
        status: "remote_pending".to_string(),
        remote: remote.summary(),
        policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
        manifest_id: String::new(),
        root_hash: String::new(),
        total_events: events.len(),
        synced_events: 0,
        uploaded_events: 0,
        uploaded_search_documents: 0,
        synced_learned_memory: 0,
        uploaded_learned_memory: 0,
        blocked_events: 0,
        blocked_learned_memory: 0,
        remote_events: 0,
        remote_learned_memory: 0,
        append_only: true,
        pii_coverage: scan_text("").pii_coverage,
        blocked: Vec::new(),
    })
}

fn status_response(
    args: &StatusArgs,
    source_filter: Option<SourceFilter>,
) -> Result<StatusResponse> {
    let db_path = default_db_path()?;
    let existed_before_command = db_path.exists();
    let store = Store::open(db_path)?;
    let info = store.info()?;
    let project_path = match &args.project {
        Some(path) => path.clone(),
        None => std::env::current_dir().context("current_dir")?,
    };
    let project = store.project_by_path(&project_path)?;
    let remote = remote_for_status()?;
    let status = status_snapshot(&store, project.as_ref(), Some(&remote))?;
    let remote_summary = remote.summary();
    let diagnostics = status_diagnostics(
        &project_path,
        &status,
        &remote_summary,
        info.schema_version,
        source_filter,
    )?;
    Ok(StatusResponse {
        command: "status".to_string(),
        store: StatusStoreDiagnostics {
            exists: Path::new(&info.db_path).exists(),
            existed_before_command,
            db_path: info.db_path,
            schema_version: info.schema_version,
            expected_schema_version: LATEST_SCHEMA_VERSION,
            schema_status: schema_status(info.schema_version).to_string(),
        },
        project: project.as_ref().map(project_status),
        remote: remote_summary,
        status,
        diagnostics,
    })
}

fn status_diagnostics(
    project_path: &Path,
    status: &StatusProjectSnapshot,
    remote: &RemoteSummary,
    schema_version: i64,
    source_filter: Option<SourceFilter>,
) -> Result<StatusDiagnostics> {
    let schema = status_schema_diagnostic(schema_version);
    let remote = status_remote_diagnostic(remote);
    let sources = status_source_diagnostics(project_path, status, source_filter)?;
    let privacy_filter = status_privacy_diagnostic();
    let summary_runner = status_summary_runner_diagnostic();
    let mut actions = Vec::new();

    if !status.linked {
        push_action(&mut actions, &link_action(project_path));
    }
    if let Some(action) = &schema.action {
        push_action(&mut actions, action);
    }
    if let Some(action) = &remote.action {
        push_action(&mut actions, action);
    }
    for source in &sources {
        if let Some(action) = &source.action {
            push_action(&mut actions, action);
        }
    }
    if let Some(action) = &privacy_filter.action {
        push_action(&mut actions, action);
    }
    if let Some(action) = &summary_runner.action {
        push_action(&mut actions, action);
    }
    if status.redaction.blocked > 0 {
        push_action(
            &mut actions,
            &format!(
                "Run `mmr redact scan --project {} --pretty`, then `mmr redact explain <event-id> --pretty` for blocked events.",
                shell_quote_path(project_path)
            ),
        );
    } else if status.sync.unsynced_events > 0 && remote.status == "available" {
        push_action(
            &mut actions,
            &format!(
                "Run `mmr sync --project {} --pretty` to upload redacted pending events.",
                shell_quote_path(project_path)
            ),
        );
    }

    Ok(StatusDiagnostics {
        schema,
        remote,
        sources,
        privacy_filter,
        summary_runner,
        actions,
    })
}

fn push_action(actions: &mut Vec<String>, action: &str) {
    if !actions.iter().any(|existing| existing == action) {
        actions.push(action.to_string());
    }
}

fn link_action(project_path: &Path) -> String {
    format!(
        "Run `cd {} && mmr init --pretty` to link and reconcile this project.",
        shell_quote_path(project_path)
    )
}

fn shell_quote_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn status_schema_diagnostic(schema_version: i64) -> StatusSchemaDiagnostic {
    let status = schema_status(schema_version);
    StatusSchemaDiagnostic {
        status: status.to_string(),
        current_version: schema_version,
        expected_version: LATEST_SCHEMA_VERSION,
        action: (status != "ok").then(|| {
            "Back up the mmr database, update mmr, and rerun the command so migrations can complete."
                .to_string()
        }),
    }
}

fn schema_status(schema_version: i64) -> &'static str {
    if schema_version == LATEST_SCHEMA_VERSION {
        "ok"
    } else {
        "mismatch"
    }
}

fn status_remote_diagnostic(remote: &RemoteSummary) -> StatusRemoteDiagnostic {
    let (status, action) = if remote.auth_status != "ok" {
        (
            "auth_failed",
            Some(
                "Set MMR_GITHUB_USER or GITHUB_USER and verify GitHub auth for github:<user>/mmr-store.",
            ),
        )
    } else if remote.available {
        ("available", None)
    } else {
        (
            "missing_remote",
            Some(
                "Run `mmr init` or `mmr sync` to create the default github:<user>/mmr-store remote.",
            ),
        )
    };
    StatusRemoteDiagnostic {
        status: status.to_string(),
        descriptor: remote.descriptor.clone(),
        backend: remote.backend.clone(),
        available: remote.available,
        auth_status: remote.auth_status.clone(),
        action: action.map(str::to_string),
    }
}

fn status_source_diagnostics(
    project_path: &Path,
    status: &StatusProjectSnapshot,
    source_filter: Option<SourceFilter>,
) -> Result<Vec<StatusSourceDiagnostic>> {
    let sources = match source_filter {
        Some(source) => vec![source],
        None => vec![
            SourceFilter::Codex,
            SourceFilter::Claude,
            SourceFilter::Cursor,
            SourceFilter::Grok,
            SourceFilter::Pi,
        ],
    };
    let mut diagnostics = Vec::new();
    for source in sources {
        diagnostics.push(status_source_diagnostic(
            project_path,
            source,
            &status.source_counts,
        )?);
    }
    for (source, event_count) in &status.source_counts {
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == *source)
        {
            continue;
        }
        diagnostics.push(StatusSourceDiagnostic {
            source: source.clone(),
            status: "active_local".to_string(),
            event_count: *event_count,
            source_root: None,
            action: None,
        });
    }
    Ok(diagnostics)
}

fn status_source_diagnostic(
    project_path: &Path,
    source: SourceFilter,
    source_counts: &BTreeMap<String, usize>,
) -> Result<StatusSourceDiagnostic> {
    let source_name = source.as_str();
    let event_count = *source_counts.get(source_name).unwrap_or(&0);
    if matches!(source, SourceFilter::Grok | SourceFilter::Pi) {
        return Ok(StatusSourceDiagnostic {
            source: source_name.to_string(),
            status: "unsupported_importer".to_string(),
            event_count,
            source_root: None,
            action: None,
        });
    }

    let Some(source_root) = default_source_root_for(source)? else {
        return Ok(StatusSourceDiagnostic {
            source: source_name.to_string(),
            status: "home_unset".to_string(),
            event_count,
            source_root: None,
            action: Some(format!(
                "Set HOME or run `mmr --source {source_name} ingest events --project {} --source-root <source-root>`.",
                shell_quote_path(project_path)
            )),
        });
    };
    let source_root_text = source_root.display().to_string();
    if source_root.exists() {
        let action = (event_count == 0).then(|| {
            format!(
                "Run `mmr --source {source_name} ingest events --project {} --source-root {}` to reconcile matching sessions.",
                shell_quote_path(project_path),
                shell_quote_path(&source_root)
            )
        });
        Ok(StatusSourceDiagnostic {
            source: source_name.to_string(),
            status: "available".to_string(),
            event_count,
            source_root: Some(source_root_text),
            action,
        })
    } else {
        Ok(StatusSourceDiagnostic {
            source: source_name.to_string(),
            status: "missing_source_root".to_string(),
            event_count,
            source_root: Some(source_root_text),
            action: Some(format!(
                "Run the {source_name} provider once, or run `mmr --source {source_name} ingest events --project {} --source-root <source-root>` with the correct source root.",
                shell_quote_path(project_path)
            )),
        })
    }
}

fn status_privacy_diagnostic() -> StatusPrivacyDiagnostic {
    let coverage = scan_text("").pii_coverage;
    let action = (coverage.status != PiiCoverageStatus::Available).then(|| {
        "Optional openai/privacy-filter is not configured; deterministic secret and coarse PII blocking still run before sync."
            .to_string()
    });
    StatusPrivacyDiagnostic {
        status: coverage.status,
        detector: coverage.detector,
        reason: coverage.reason,
        action,
    }
}

fn status_summary_runner_diagnostic() -> StatusSummaryRunnerDiagnostic {
    let configured = summarize_api_key_configured();
    let config_path = config::mmr_config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~/.config/mmr/config.json".to_string());
    StatusSummaryRunnerDiagnostic {
        backend: "openai-compatible".to_string(),
        status: if configured {
            "configured"
        } else {
            "missing_api_key"
        }
        .to_string(),
        endpoint: summarize_endpoint_for_status(),
        model: effective_summariser_model(None),
        config_file: config_path.clone(),
        api_key_env: vec!["OPENAI_API_KEY".to_string()],
        action: (!configured).then(|| {
            format!(
                "Set summarize.apiKey or summarize.apiKeyEnv in {config_path}, or set OPENAI_API_KEY; optionally set summarize.baseUrl and summarize.model."
            )
        }),
    }
}

fn reconcile_default_sources(
    store: &mut Store,
    project: &ProjectRecord,
    source_filter: Option<SourceFilter>,
) -> Result<Vec<SourceImportStatus>> {
    let sources = match source_filter {
        Some(source) => vec![source],
        None => vec![
            SourceFilter::Codex,
            SourceFilter::Claude,
            SourceFilter::Cursor,
        ],
    };
    let mut reports = Vec::new();
    for source in sources {
        let source_name = source.as_str();
        let Some(source_root) = default_source_root_for(source)? else {
            reports.push(SourceImportStatus {
                source: source_name.to_string(),
                status: "unsupported".to_string(),
                discovered_sessions: 0,
                imported_events: 0,
                warnings: vec![format!("{source_name} source import is not implemented")],
            });
            continue;
        };
        if !source_root.exists() {
            reports.push(SourceImportStatus {
                source: source_name.to_string(),
                status: "skipped".to_string(),
                discovered_sessions: 0,
                imported_events: 0,
                warnings: vec![format!("source root not found for {source_name}")],
            });
            continue;
        }
        let root = SourceDiscoveryRoot {
            project_path: PathBuf::from(&project.canonical_path),
            source_root,
        };
        let report = match source {
            SourceFilter::Codex => {
                import_with_adapter(&CodexAdapter::new(), store, &project.id, &root)?
            }
            SourceFilter::Claude => {
                import_with_adapter(&ClaudeAdapter::new(), store, &project.id, &root)?
            }
            SourceFilter::Cursor => {
                import_with_adapter(&CursorAdapter::new(), store, &project.id, &root)?
            }
            SourceFilter::Grok | SourceFilter::Pi => {
                unreachable!("unsupported sources handled above")
            }
        };
        reports.push(SourceImportStatus {
            source: report.source,
            status: "imported".to_string(),
            discovered_sessions: report.discovered_sessions,
            imported_events: report.imported_events,
            warnings: public_import_warnings(report.warnings),
        });
    }
    Ok(reports)
}

fn public_import_warnings(warnings: Vec<String>) -> Vec<String> {
    warnings
        .into_iter()
        .map(|warning| {
            if warning.starts_with('/') {
                warning
                    .split_once(": ")
                    .map(|(_, message)| message.to_string())
                    .unwrap_or_else(|| "source import warning".to_string())
            } else {
                warning
            }
        })
        .collect()
}

fn default_source_root_for(source: SourceFilter) -> Result<Option<PathBuf>> {
    let home = match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => return Ok(None),
    };
    Ok(match source {
        SourceFilter::Codex => Some(home.join(".codex")),
        SourceFilter::Claude => Some(home.join(".claude")),
        SourceFilter::Cursor => Some(home.join(".cursor")),
        SourceFilter::Grok | SourceFilter::Pi => None,
    })
}

fn rebuild_search_documents(
    store: &Store,
    project: &ProjectRecord,
    source_filter: Option<SourceFilter>,
) -> Result<usize> {
    let events =
        store.events_for_project(&project.id, source_filter.map(SourceFilter::as_str), None)?;
    for event in &events {
        store.upsert_search_document(event)?;
    }
    Ok(events.len())
}

fn status_snapshot(
    store: &Store,
    project: Option<&ProjectRecord>,
    remote: Option<&crate::sync::FakeGithubRemote>,
) -> Result<StatusProjectSnapshot> {
    let Some(project) = project else {
        return Ok(StatusProjectSnapshot {
            linked: false,
            sync_status: "unlinked".to_string(),
            events_total: 0,
            source_counts: BTreeMap::new(),
            sync_status_counts: BTreeMap::new(),
            redaction: StatusRedactionSnapshot {
                policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
                redacted_or_synced: 0,
                blocked: 0,
                pending: 0,
            },
            sync: StatusSyncSnapshot {
                remote_events: 0,
                local_manifests: 0,
                latest_manifest_id: None,
                blocked_events: 0,
                unsynced_events: 0,
            },
        });
    };

    let events = store.events_for_project(&project.id, None, None)?;
    let mut source_counts = BTreeMap::new();
    let mut sync_status_counts = BTreeMap::new();
    for event in &events {
        *source_counts.entry(event.source.clone()).or_insert(0) += 1;
        *sync_status_counts
            .entry(event.sync_status.clone())
            .or_insert(0) += 1;
    }
    let blocked_events = *sync_status_counts.get("blocked").unwrap_or(&0);
    let synced_events = *sync_status_counts.get("synced").unwrap_or(&0);
    let redacted_events = *sync_status_counts.get("redacted").unwrap_or(&0);
    let pending_events = events
        .len()
        .saturating_sub(blocked_events + synced_events + redacted_events);
    let unsynced_events = events.len().saturating_sub(synced_events + blocked_events);
    let manifests = store.sync_manifests_for_project(&project.id)?;
    let remote_summary = remote.map(|remote| remote.summary());
    let remote_auth_failed = remote_summary
        .as_ref()
        .is_some_and(|summary| summary.auth_status != "ok");
    let remote_available = remote_summary
        .as_ref()
        .is_some_and(|summary| summary.available && summary.auth_status == "ok");
    let remote_events = match remote {
        Some(remote) if remote_available => remote.count_events(project).unwrap_or(0),
        _ => 0,
    };
    let remote_required = synced_events > 0 || !manifests.is_empty();
    let sync_status = if remote_auth_failed {
        "remote_unavailable"
    } else if blocked_events > 0 {
        "blocked"
    } else if remote_required && !remote_available {
        "remote_unavailable"
    } else if remote_available && remote_events < synced_events {
        "remote_missing"
    } else if unsynced_events > 0 {
        "pending"
    } else {
        "synced"
    };

    Ok(StatusProjectSnapshot {
        linked: true,
        sync_status: sync_status.to_string(),
        events_total: events.len(),
        source_counts,
        sync_status_counts,
        redaction: StatusRedactionSnapshot {
            policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
            redacted_or_synced: redacted_events + synced_events,
            blocked: blocked_events,
            pending: pending_events,
        },
        sync: StatusSyncSnapshot {
            remote_events,
            local_manifests: manifests.len(),
            latest_manifest_id: manifests.first().map(|manifest| manifest.id.clone()),
            blocked_events,
            unsynced_events,
        },
    })
}

fn project_status(project: &ProjectRecord) -> ProjectStatus {
    ProjectStatus {
        id: project.id.clone(),
        display_name: project.display_name.clone(),
        path_hash: content_hash(&project.canonical_path),
    }
}

fn now_rfc3339() -> Result<String> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .context("format timestamp")
}

#[derive(Debug, Serialize)]
struct IngestEventsResponse {
    project_id: String,
    source: String,
    discovered_sessions: usize,
    imported_events: usize,
    warnings: Vec<String>,
    event_ids: Vec<String>,
}

fn ingest_events_response(
    args: &IngestEventsArgs,
    source_filter: Option<SourceFilter>,
) -> Result<IngestEventsResponse> {
    let mut store = Store::open_default()?;
    let project = store.ensure_project_link(&args.project)?;
    let source_root = match &args.source_root {
        Some(source_root) => source_root.clone(),
        None => default_import_source_root(source_filter)?,
    };
    let root = SourceDiscoveryRoot {
        project_path: args.project.clone(),
        source_root,
    };
    let report = match source_filter {
        Some(SourceFilter::Codex) => {
            import_with_adapter(&CodexAdapter::new(), &mut store, &project.id, &root)?
        }
        Some(SourceFilter::Claude) => {
            import_with_adapter(&ClaudeAdapter::new(), &mut store, &project.id, &root)?
        }
        Some(SourceFilter::Cursor) => {
            import_with_adapter(&CursorAdapter::new(), &mut store, &project.id, &root)?
        }
        _ => bail!(
            "`mmr ingest events` requires `--source codex`, `--source claude`, or `--source cursor`"
        ),
    };

    Ok(IngestEventsResponse {
        project_id: project.id,
        source: report.source,
        discovered_sessions: report.discovered_sessions,
        imported_events: report.imported_events,
        warnings: report.warnings,
        event_ids: report.event_ids,
    })
}

fn import_with_adapter<A: SourceAdapter>(
    adapter: &A,
    store: &mut Store,
    project_id: &str,
    root: &SourceDiscoveryRoot,
) -> Result<crate::capture::ReconcileReport> {
    Reconciler::new(adapter).reconcile(store, project_id, root)
}

fn default_import_source_root(source_filter: Option<SourceFilter>) -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set; pass --source-root"))?;
    match source_filter {
        Some(SourceFilter::Codex) => Ok(home.join(".codex")),
        Some(SourceFilter::Claude) => Ok(home.join(".claude")),
        Some(SourceFilter::Cursor) => Ok(home.join(".cursor")),
        _ => bail!(
            "`mmr ingest events` requires `--source codex`, `--source claude`, or `--source cursor`"
        ),
    }
}

#[derive(Debug, Serialize)]
struct ExportTreeResponse {
    format: String,
    output_dir: String,
    total_files: usize,
    files: Vec<ExportTreeFile>,
}

#[derive(Debug, Serialize)]
struct ExportTreeFile {
    path: String,
    event_id: String,
    citation: String,
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

fn export_tree_project_response(
    project: Option<String>,
    output_dir: Option<PathBuf>,
    source_filter: Option<SourceFilter>,
) -> Result<ExportTreeResponse> {
    let store = Store::open_default()?;
    let project_path = project.as_deref().map(PathBuf::from);
    let project = linked_project(&store, project_path.as_deref())?;
    let events =
        store.events_for_project(&project.id, source_filter.map(SourceFilter::as_str), None)?;
    export_tree_events_response(events, output_dir, &project.id)
}

fn export_tree_events_response(
    events: Vec<EventRecord>,
    output_dir: Option<PathBuf>,
    scope_key: &str,
) -> Result<ExportTreeResponse> {
    let base_output_dir =
        output_dir.ok_or_else(|| anyhow::anyhow!("--output-dir is required with --format tree"))?;
    fs::create_dir_all(&base_output_dir).with_context(|| {
        format!(
            "create export output directory {}",
            base_output_dir.display()
        )
    })?;

    let store = Store::open_default()?;
    let run_dir = base_output_dir.join(format!(
        "mmr-tree-{}",
        sanitize_path_component(&content_hash(&format!(
            "{}:{}:{}",
            scope_key,
            events.len(),
            now_rfc3339()?
        )))
    ));
    fs::create_dir_all(&run_dir)
        .with_context(|| format!("create export run directory {}", run_dir.display()))?;
    let mut files = Vec::new();

    for event in events {
        let search_document = store.upsert_search_document(&event)?;
        let relative_path = PathBuf::from(sanitize_path_component(&event.source))
            .join(sanitize_path_component(&event.session_id))
            .join(format!("{}.md", sanitize_path_component(&event.id)));
        let full_path = run_dir.join(&relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create export subdirectory {}", parent.display()))?;
        }
        let contents = format!(
            "# mmr event\n\ncitation: {}\nsource: {}\nsession_id: {}\nevent_id: {}\nevent_type: {}\nrole: {}\ntimestamp: {}\n\n{}\n",
            search_document.citation,
            event.source,
            event.session_id,
            event.id,
            event.event_type,
            event.role,
            event.timestamp,
            search_document.document_text
        );
        fs::write(&full_path, contents)
            .with_context(|| format!("write export file {}", full_path.display()))?;
        files.push(ExportTreeFile {
            path: full_path.to_string_lossy().into_owned(),
            event_id: event.id,
            citation: search_document.citation,
        });
    }

    Ok(ExportTreeResponse {
        format: "tree".to_string(),
        output_dir: run_dir.to_string_lossy().into_owned(),
        total_files: files.len(),
        files,
    })
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Debug, Serialize)]
struct RedactScanResponse {
    project_id: String,
    policy_id: String,
    events_scanned: usize,
    passed: usize,
    blocked: usize,
    pii_coverage: PiiCoverage,
    events: Vec<RedactedEventSummary>,
}

#[derive(Debug, Serialize)]
struct RedactExplainResponse {
    event_id: String,
    policy_id: String,
    status: String,
    blocking_findings: i64,
    spans: Vec<RedactionSpanResponse>,
    redacted_text: String,
}

#[derive(Debug, Serialize)]
struct SyncDryRunResponse {
    dry_run: bool,
    project_id: String,
    remote: String,
    policy_id: String,
    total_events: usize,
    syncable_events: usize,
    blocked_events: usize,
    pii_coverage: PiiCoverage,
    events: Vec<SyncDryRunEvent>,
}

#[derive(Debug, Serialize)]
struct RedactedEventSummary {
    event_id: String,
    status: String,
    span_count: usize,
    blocking_findings: usize,
    kinds: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RedactionSpanResponse {
    kind: String,
    start_byte: usize,
    end_byte: usize,
    replacement: String,
    confidence: f64,
    blocks_sync: bool,
}

#[derive(Debug, Serialize)]
struct SyncDryRunEvent {
    event_id: String,
    source: String,
    status: String,
    would_sync: bool,
    payload_preview: Option<String>,
    blocked_reasons: Vec<String>,
}

fn redact_response(
    args: &RedactArgs,
    source_filter: Option<SourceFilter>,
) -> Result<serde_json::Value> {
    match &args.command {
        RedactCommand::Scan { project } => {
            let mut store = Store::open_default()?;
            let project = linked_project(&store, project.as_deref())?;
            let response = scan_project_response(&mut store, &project, source_filter)?;
            serde_json::to_value(response).context("serialize redact scan response")
        }
        RedactCommand::Explain { event_id } => {
            let store = Store::open_default()?;
            let event = store.event_by_id(event_id)?;
            let run = store
                .latest_redaction_run_for_event(event_id)?
                .ok_or_else(|| anyhow::anyhow!("no redaction run found for event: {event_id}"))?;
            let spans = store.redaction_spans_for_run(&run.id)?;
            let redacted_text = crate::redaction::apply_redactions(
                &event.content_text,
                &spans
                    .iter()
                    .map(redaction_finding_from_record)
                    .collect::<Vec<_>>(),
            );
            let response = RedactExplainResponse {
                event_id: event.id,
                policy_id: run.policy_id,
                status: run.status,
                blocking_findings: run.blocking_findings,
                spans: spans.iter().map(redaction_span_response).collect(),
                redacted_text,
            };
            serde_json::to_value(response).context("serialize redact explain response")
        }
    }
}

fn sync_response(
    args: &SyncArgs,
    source_filter: Option<SourceFilter>,
) -> Result<serde_json::Value> {
    let store = Store::open_default()?;
    let project = linked_project(&store, args.project.as_deref())?;
    if !args.dry_run {
        drop(store);
        let mut store = Store::open_default()?;
        let remote = remote_for_operations()?;
        hydrate_project(&mut store, &project, &remote)?;
        reconcile_default_sources(&mut store, &project, source_filter)?;
        rebuild_search_documents(&store, &project, source_filter)?;
        let response = sync_project(
            &mut store,
            &project,
            &remote,
            source_filter.map(SourceFilter::as_str),
        )?;
        return serde_json::to_value(response).context("serialize sync response");
    }

    let events =
        store.events_for_project(&project.id, source_filter.map(SourceFilter::as_str), None)?;

    let mut sync_events = Vec::new();
    let mut syncable_events = 0;
    let mut blocked_events = 0;
    let mut pii_coverage = None;
    for event in events {
        let outcome = scan_text(&event.content_text);
        pii_coverage = Some(outcome.pii_coverage.clone());
        let would_sync = dry_run_allows_sync(&event, &outcome);
        if would_sync {
            syncable_events += 1;
        } else {
            blocked_events += 1;
        }
        let blocked_reasons = dry_run_blocked_reasons(&event, &outcome);
        let status = if safe_projection_blocker(&event).is_some() {
            "requires_safe_projection"
        } else if outcome.blocks_sync {
            "blocked"
        } else if would_sync {
            "passed"
        } else {
            "degraded_policy"
        };
        sync_events.push(SyncDryRunEvent {
            event_id: event.id,
            source: event.source,
            status: status.to_string(),
            would_sync,
            payload_preview: if would_sync {
                Some(outcome.redacted_text)
            } else {
                None
            },
            blocked_reasons,
        });
    }

    let response = SyncDryRunResponse {
        dry_run: true,
        project_id: project.id,
        remote: "github:<authenticated-user>/mmr-store".to_string(),
        policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
        total_events: sync_events.len(),
        syncable_events,
        blocked_events,
        pii_coverage: pii_coverage.unwrap_or_else(|| scan_text("").pii_coverage),
        events: sync_events,
    };
    serde_json::to_value(response).context("serialize sync dry-run response")
}

fn linked_project(store: &Store, project: Option<&Path>) -> Result<ProjectRecord> {
    let path = match project {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("current_dir")?,
    };
    store.project_by_path(&path)?.ok_or_else(|| {
        anyhow::anyhow!(
            "project is not linked; run `mmr init` before redaction or pass a linked --project"
        )
    })
}

fn scan_project_response(
    store: &mut Store,
    project: &ProjectRecord,
    source_filter: Option<SourceFilter>,
) -> Result<RedactScanResponse> {
    let events =
        store.events_for_project(&project.id, source_filter.map(SourceFilter::as_str), None)?;
    let mut summaries = Vec::with_capacity(events.len());
    let mut passed = 0;
    let mut blocked = 0;
    let mut pii_coverage = None;

    for event in events {
        let outcome = scan_text(&event.content_text);
        pii_coverage = Some(outcome.pii_coverage.clone());
        let spans = outcome
            .findings
            .iter()
            .map(new_redaction_span_from_finding)
            .collect::<Vec<_>>();
        let status = if outcome.blocks_sync {
            blocked += 1;
            "blocked"
        } else {
            passed += 1;
            "passed"
        };
        store.record_redaction_result(&event.id, DEFAULT_REDACTION_POLICY_ID, status, &spans)?;
        summaries.push(redacted_event_summary(&event, status, &outcome.findings));
    }

    Ok(RedactScanResponse {
        project_id: project.id.clone(),
        policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
        events_scanned: summaries.len(),
        passed,
        blocked,
        pii_coverage: pii_coverage.unwrap_or_else(|| scan_text("").pii_coverage),
        events: summaries,
    })
}

fn dry_run_allows_sync(event: &EventRecord, outcome: &RedactionOutcome) -> bool {
    safe_projection_blocker(event).is_none()
        && !outcome.blocks_sync
        && outcome.pii_coverage.status == PiiCoverageStatus::Available
}

fn dry_run_blocked_reasons(event: &EventRecord, outcome: &RedactionOutcome) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(reason) = safe_projection_blocker(event) {
        reasons.push(reason.to_string());
    }
    let blocking_findings = outcome
        .findings
        .iter()
        .filter(|finding| finding.blocks_sync)
        .count();
    if blocking_findings > 0 {
        reasons.push(format!(
            "{blocking_findings} deterministic secret finding(s) under policy {DEFAULT_REDACTION_POLICY_ID}"
        ));
    }
    if outcome.pii_coverage.status == PiiCoverageStatus::Degraded {
        reasons.push(outcome.pii_coverage.reason.clone());
    }
    reasons
}

fn redacted_event_summary(
    event: &EventRecord,
    status: &str,
    findings: &[RedactionFinding],
) -> RedactedEventSummary {
    let mut kinds = findings
        .iter()
        .map(|finding| finding.kind.clone())
        .collect::<Vec<_>>();
    kinds.sort();
    kinds.dedup();
    RedactedEventSummary {
        event_id: event.id.clone(),
        status: status.to_string(),
        span_count: findings.len(),
        blocking_findings: findings
            .iter()
            .filter(|finding| finding.blocks_sync)
            .count(),
        kinds,
    }
}

fn new_redaction_span_from_finding(finding: &RedactionFinding) -> NewRedactionSpan {
    NewRedactionSpan {
        kind: finding.kind.clone(),
        start_byte: finding.start_byte,
        end_byte: finding.end_byte,
        replacement: finding.replacement.clone(),
        confidence: finding.confidence,
        blocks_sync: finding.blocks_sync,
    }
}

fn redaction_span_response(span: &crate::store::RedactionSpanRecord) -> RedactionSpanResponse {
    RedactionSpanResponse {
        kind: span.kind.clone(),
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        replacement: span.replacement.clone(),
        confidence: span.confidence,
        blocks_sync: span.blocks_sync,
    }
}

fn redaction_finding_from_record(span: &crate::store::RedactionSpanRecord) -> RedactionFinding {
    RedactionFinding {
        kind: span.kind.clone(),
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        replacement: span.replacement.clone(),
        confidence: span.confidence,
        blocks_sync: span.blocks_sync,
    }
}

#[derive(Debug, Serialize)]
struct DbInfoResponse {
    db_path: String,
    schema_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_count: Option<i64>,
}

fn db_info_response(project: Option<PathBuf>, smoke_event: bool) -> Result<DbInfoResponse> {
    let mut store = Store::open_default()?;
    let info = store.info()?;
    let mut response = DbInfoResponse {
        db_path: info.db_path,
        schema_version: info.schema_version,
        project_id: None,
        event_count: None,
    };

    if project.is_some() || smoke_event {
        let project_path = match project {
            Some(project) => project,
            None => std::env::current_dir().context("current_dir")?,
        };
        let project = store.ensure_project_link(&project_path)?;
        response.project_id = Some(project.id.clone());

        if smoke_event {
            let event = NewEvent::new(
                "dev",
                "__db-info-smoke",
                "smoke",
                "user",
                "2026-05-24T00:00:00Z",
                "synthetic store smoke event",
                "dev-smoke-v1",
            )
            .with_source_event_id("synthetic-event-1");
            store.insert_event(&project.id, &event)?;
            response.event_count = Some(
                store
                    .events_for_project(&project.id, Some("dev"), Some("__db-info-smoke"))?
                    .len() as i64,
            );
        }
    }

    Ok(response)
}

fn effective_source(cli_source: Option<SourceFilter>) -> Option<SourceFilter> {
    cli_source.or_else(default_source_from_env)
}

fn default_source_from_env() -> Option<SourceFilter> {
    std::env::var(ENV_DEFAULT_SOURCE)
        .ok()
        .and_then(|value| parse_source_filter_env(&value))
}

fn parse_source_filter_env(value: &str) -> Option<SourceFilter> {
    SourceFilter::from_name(value.trim().to_ascii_lowercase().as_str())
}

fn effective_summariser_model(cli_model: Option<&str>) -> String {
    config::resolve_summarize_settings(cli_model)
        .map(|settings| settings.model)
        .unwrap_or_else(|_| DEFAULT_SUMMARISER_MODEL.to_string())
}

fn effective_compact_model(cli_model: Option<&str>) -> String {
    cli_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(default_compact_model_from_env)
        .unwrap_or_else(|| compact::default_compact_model().to_string())
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

fn default_compact_model_from_env() -> Option<String> {
    std::env::var(ENV_COMPACT_MODEL)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
        Some(content_hash(&remotes.join("\n")))
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
    cli_source: Option<SourceFilter>,
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
            cli_source,
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

fn format_remember_response(
    response: &RememberResponse,
    output_format: RememberOutputFormatArg,
    pretty: bool,
) -> Result<String> {
    match output_format {
        RememberOutputFormatArg::Json => serialize(response, pretty),
        RememberOutputFormatArg::Md => Ok(remember_response_to_markdown(response)),
    }
}

fn format_compact_response(
    response: &CompactResponse,
    output_format: CompactOutputFormatArg,
    pretty: bool,
) -> Result<String> {
    match output_format {
        CompactOutputFormatArg::Json => serialize(response, pretty),
        CompactOutputFormatArg::Md => Ok(compact_response_to_markdown(response)),
    }
}

fn remember_response_to_markdown(response: &RememberResponse) -> String {
    if response.text.trim().is_empty() {
        "(No continuity brief returned.)"
    } else {
        response.text.trim()
    }
    .to_string()
}

fn compact_response_to_markdown(response: &CompactResponse) -> String {
    if response.output.trim().is_empty() {
        "(No compacted transcript returned.)"
    } else {
        response.output.trim()
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_project_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "summarize",
            "project",
            "--project",
            "/Users/test/proj",
            "--model",
            "openrouter/auto",
            "-O",
            "json",
        ])
        .expect("summarize project should parse");
        let Commands::Summarize(args) = parsed.command else {
            panic!("expected summarize command");
        };
        let SummarizeCommand::Project(project) = args.command else {
            panic!("expected summarize project command");
        };
        assert_eq!(project.project.as_deref(), Some("/Users/test/proj"));
        assert_eq!(project.runner.model.as_deref(), Some("openrouter/auto"));
        assert_eq!(project.runner.output_format, RememberOutputFormatArg::Json);
    }

    #[test]
    fn summarize_session_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "summarize",
            "session",
            "sess-123",
            "--project",
            "/Users/test/proj",
        ])
        .expect("summarize session should parse");
        let Commands::Summarize(args) = parsed.command else {
            panic!("expected summarize command");
        };
        let SummarizeCommand::Session(session) = args.command else {
            panic!("expected summarize session command");
        };
        assert_eq!(session.session_id, "sess-123");
        assert_eq!(session.project.as_deref(), Some("/Users/test/proj"));
    }

    #[test]
    fn summarize_project_limit_offset_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "summarize",
            "project",
            "--limit",
            "10",
            "--offset",
            "5",
        ])
        .expect("summarize project limit/offset should parse");
        let Commands::Summarize(args) = parsed.command else {
            panic!("expected summarize command");
        };
        let SummarizeCommand::Project(project) = args.command else {
            panic!("expected summarize project command");
        };
        assert_eq!(project.limit, Some(10));
        assert_eq!(project.offset, 5);
    }

    #[test]
    fn compact_project_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "compact",
            "project",
            "--project",
            "/Users/test/proj",
            "--query",
            "active task",
            "--compression-ratio",
            "0.4",
            "--preserve-recent",
            "3",
            "--no-line-ranges",
            "--no-markers",
            "--model",
            "morph-compactor",
            "-O",
            "md",
        ])
        .expect("compact project should parse");
        let Commands::Compact(args) = parsed.command else {
            panic!("expected compact command");
        };
        let CompactCommand::Project(project) = args.command else {
            panic!("expected compact project command");
        };
        assert_eq!(project.project.as_deref(), Some("/Users/test/proj"));
        assert_eq!(project.runner.query.as_deref(), Some("active task"));
        assert_eq!(project.runner.compression_ratio, Some(0.4));
        assert_eq!(project.runner.preserve_recent, Some(3));
        assert!(project.runner.no_line_ranges);
        assert!(project.runner.no_markers);
        assert_eq!(project.runner.model.as_deref(), Some("morph-compactor"));
        assert_eq!(project.runner.output_format, CompactOutputFormatArg::Md);
    }

    #[test]
    fn compact_session_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "compact",
            "session",
            "sess-123",
            "--project",
            "/Users/test/proj",
        ])
        .expect("compact session should parse");
        let Commands::Compact(args) = parsed.command else {
            panic!("expected compact command");
        };
        let CompactCommand::Session(session) = args.command else {
            panic!("expected compact session command");
        };
        assert_eq!(session.session_id, "sess-123");
        assert_eq!(session.project.as_deref(), Some("/Users/test/proj"));
    }

    #[test]
    fn old_summary_and_remember_do_not_parse() {
        assert!(Cli::try_parse_from(["mmr", "summary", "all"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "remember", "all"]).is_err());
    }

    #[test]
    fn parse_source_filter_env_accepts_supported_values() {
        assert_eq!(parse_source_filter_env("codex"), Some(SourceFilter::Codex));
        assert_eq!(
            parse_source_filter_env("CLAUDE"),
            Some(SourceFilter::Claude)
        );
        assert_eq!(
            parse_source_filter_env("cursor"),
            Some(SourceFilter::Cursor)
        );
        assert_eq!(
            parse_source_filter_env("CURSOR"),
            Some(SourceFilter::Cursor)
        );
        assert_eq!(parse_source_filter_env("grok"), Some(SourceFilter::Grok));
        assert_eq!(parse_source_filter_env("GROK"), Some(SourceFilter::Grok));
        assert_eq!(parse_source_filter_env("pi"), Some(SourceFilter::Pi));
        assert_eq!(parse_source_filter_env("PI"), Some(SourceFilter::Pi));
        assert_eq!(parse_source_filter_env(""), None);
        assert_eq!(parse_source_filter_env("invalid"), None);
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
    fn remember_markdown_transformation_includes_summary_only() {
        let response = RememberResponse {
            backend: "openai-compatible".to_string(),
            model: "test-model".to_string(),
            text: "# Continuity Brief\n\nSummary body".to_string(),
        };

        let markdown = remember_response_to_markdown(&response);
        assert!(markdown.contains("# Continuity Brief"));
        assert!(markdown.contains("Summary body"));
        assert!(!markdown.contains("Interaction ID:"));
        assert!(!markdown.contains("Thread ID:"));
    }

    #[test]
    fn remember_markdown_transformation_handles_empty_values() {
        let response = RememberResponse {
            backend: "openai-compatible".to_string(),
            model: "test-model".to_string(),
            text: "  ".to_string(),
        };

        let markdown = remember_response_to_markdown(&response);
        assert!(markdown.contains("(No continuity brief returned.)"));
        assert!(!markdown.contains("Interaction ID:"));
        assert!(!markdown.contains("Thread ID:"));
    }

    #[test]
    fn remember_markdown_transformation_trims_outer_whitespace() {
        let response = RememberResponse {
            backend: "openai-compatible".to_string(),
            model: "test-model".to_string(),
            text: "\n  line one\nline two  \n".to_string(),
        };

        let markdown = remember_response_to_markdown(&response);
        assert!(markdown.contains("line one\nline two"));
        assert!(!markdown.contains("id-1"));
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
    fn sync_command_is_rejected() {
        assert!(Cli::try_parse_from(["mmr", "sync", "status"]).is_err());
    }

    #[test]
    fn note_command_parses_inline_text() {
        let parsed = Cli::try_parse_from(["mmr", "note", "decision:", "use", "fixtures"])
            .expect("note should parse");
        let Commands::Note { text } = parsed.command else {
            panic!("expected note command");
        };
        assert_eq!(text, vec!["decision:", "use", "fixtures"]);
    }

    #[test]
    fn find_command_parses_json_and_line_formats() {
        let find = Cli::try_parse_from([
            "mmr",
            "find",
            "panic",
            "--role",
            "assistant",
            "-i",
            "--format",
            "line",
        ])
        .expect("find should parse");
        let Commands::Find(args) = find.command else {
            panic!("expected find command");
        };
        assert_eq!(args.query, "panic");
        assert_eq!(args.role.as_deref(), Some("assistant"));
        assert!(args.ignore_case);
        assert_eq!(args.format, FindFormatArg::Line);

        assert!(Cli::try_parse_from(["mmr", "rg", "panic"]).is_err());
        assert!(Cli::try_parse_from(["mmr", "search", "decision"]).is_err());
    }

    #[test]
    fn ingest_events_command_parses_with_global_source_after_subcommand() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "ingest",
            "events",
            "--source",
            "codex",
            "--project",
            "/tmp/project",
            "--source-root",
            "/tmp/.codex",
        ])
        .expect("ingest events should parse");
        assert_eq!(parsed.source, Some(SourceFilter::Codex));
        let Commands::Ingest(args) = parsed.command else {
            panic!("expected ingest command");
        };
        let IngestCommand::Events(args) = args.command;
        assert_eq!(args.project, PathBuf::from("/tmp/project"));
        assert_eq!(args.source_root, Some(PathBuf::from("/tmp/.codex")));
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
    fn tool_results_need_safe_projection_even_after_passing_redaction() {
        let event = EventRecord {
            id: "evt:v1:tool-result".to_string(),
            project_id: "proj:v1:test".to_string(),
            session_id: "session:v1:test".to_string(),
            source: "codex".to_string(),
            source_session_id: "codex-session".to_string(),
            source_event_id: Some("out-1".to_string()),
            event_type: "tool_result".to_string(),
            role: "tool".to_string(),
            timestamp: "2026-05-24T12:00:00Z".to_string(),
            content_text: "benign output".to_string(),
            content_hash: "hash".to_string(),
            parent_hash: None,
            parser_version: "codex-rollout-v1".to_string(),
            raw_local_ref: Some("/tmp/codex.jsonl:1".to_string()),
            sync_status: "redacted".to_string(),
        };
        let outcome = RedactionOutcome {
            findings: Vec::new(),
            redacted_text: event.content_text.clone(),
            blocks_sync: false,
            pii_coverage: PiiCoverage {
                status: PiiCoverageStatus::Available,
                detector: "deterministic-pii-rules".to_string(),
                reason: "test detector".to_string(),
            },
        };

        assert!(!dry_run_allows_sync(&event, &outcome));
        assert!(
            dry_run_blocked_reasons(&event, &outcome)
                .iter()
                .any(|reason| reason.contains("dedicated safe sync projection"))
        );
    }

    #[test]
    fn redact_scan_command_parses() {
        let parsed = Cli::try_parse_from(["mmr", "redact", "scan"]).expect("redact scan parses");
        let Commands::Redact(args) = parsed.command else {
            panic!("expected redact command");
        };
        assert!(matches!(
            args.command,
            RedactCommand::Scan { project: None }
        ));
    }

    #[test]
    fn sync_dry_run_command_parses() {
        let parsed = Cli::try_parse_from(["mmr", "sync", "--dry-run"]).expect("sync parses");
        let Commands::Sync(args) = parsed.command else {
            panic!("expected sync command");
        };
        assert!(args.dry_run);
    }
}
