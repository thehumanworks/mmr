use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
use crate::dream::{
    DreamEvidence, DreamEvidenceMode, OmittedDreamEvidence, build_evidence_bundle,
    build_source_evidence_bundle,
};
use crate::messages::service::{
    MessageIndexRange, MessageQueryOptions, QueryService, SessionAxis, SessionSelectionError,
};
use crate::peer::{
    PEER_PROTOCOL_VERSION, PeerListProjectsRequest, PeerListSessionsRequest, PeerProjectIdentity,
    PeerProjectRequest, PeerReadSessionRequest, PeerReadSourceRequest, PeerRecallRequest,
    PeerRequestLimits, PeerStatusResponse, PeerTeleportPackRequest, peer_status, run_peer_json,
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
    serve_session, write_bundle,
};
use crate::types::{
    ApiMessage, ApiMessageOrigin, ApiMessagesResponse, ApiPeerResult, ApiProject,
    ApiProjectsResponse, ApiSession, ApiSessionsResponse, CompactResponse, RememberRequest,
    RememberResponse, RememberSelection, SortBy, SortOptions, SortOrder, SourceFilter,
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
    after_help = "Examples:\n  mmr init\n  mmr status --pretty\n  mmr list projects --pretty\n  mmr list sessions --remote mini --project /path/to/project\n  mmr recall --remote mini --pretty\n  mmr read session <session-id> --pretty\n  mmr read project --remote mini\n  mmr read project --format tree --output-dir /tmp/mmr-tree\n  mmr share session latest --to user@host\n  mmr import session --from mini --session latest --project /path/to/project --read-only\n  mmr --source codex ingest events --project /path/to/project\n  mmr find \"migration append-only\" --format line\n  mmr summarize project --project /path/to/project\n  mmr compact project --project /path/to/project --query \"current task\"\n  mmr assimilate project --pretty\n  mmr skill load\n  mmr skill install --local\n  mmr mcp --transport stdio\n  mmr mcp --transport http\n  mmr sync --pretty\n\nOutput:\n  Commands emit machine-readable JSON on stdout unless an explicit stream format such as --format line is selected. Use --pretty for indented JSON. `mmr mcp --transport stdio` reserves stdout for MCP protocol frames."
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
    /// Return prompt/runbook/evidence for memory assimilation
    Assimilate(AssimilateArgs),
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
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Read source-scoped history across all projects for one harness
    Source(ReadSourceArgs),
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
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ReadSourceArgs {
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
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Produce harness-specific context across all projects
    Source(ContextSourceArgs),
}

#[derive(Args, Debug)]
pub struct ContextProjectArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long)]
    project: Option<String>,
    /// Maximum number of recent messages to include
    #[arg(long, default_value_t = 100)]
    limit: usize,
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ContextSourceArgs {
    /// Maximum number of recent messages to include
    #[arg(long, default_value_t = 200)]
    limit: usize,
    /// Query this explicit SSH target in addition to local history
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Run a stateless summary over all history from one harness/source
    Source(SummarizeSourceArgs),
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
    /// Query this explicit SSH target in addition to local history before summarizing
    #[arg(long = "remote")]
    remotes: Vec<String>,
    #[command(flatten)]
    runner: SummaryRunnerArgs,
}

#[derive(Args, Debug)]
pub struct SummarizeSourceArgs {
    /// Query this explicit SSH target in addition to local history before summarizing
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Query this explicit SSH target in addition to local history before summarizing
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Compact all history from one harness/source with Morph Compact
    Source(CompactSourceArgs),
    /// Compact one explicit session with Morph Compact
    Session(CompactSessionArgs),
}

#[derive(Args, Debug)]
pub struct CompactProjectArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long, short = 'p')]
    project: Option<String>,
    /// Query this explicit SSH target in addition to local history before compacting
    #[arg(long = "remote")]
    remotes: Vec<String>,
    #[command(flatten)]
    runner: CompactRunnerArgs,
}

#[derive(Args, Debug)]
pub struct CompactSourceArgs {
    /// Query this explicit SSH target in addition to local history before compacting
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
    /// Query this explicit SSH target in addition to local history before compacting
    #[arg(long = "remote")]
    remotes: Vec<String>,
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
pub struct AssimilateArgs {
    #[command(subcommand)]
    command: AssimilateCommand,
}

#[derive(Subcommand, Debug)]
pub enum AssimilateCommand {
    /// Return project prompt/runbook/output contract/evidence
    Project(AssimilateProjectArgs),
    /// Return source-wide prompt/runbook/output contract/evidence
    Source(AssimilateSourceArgs),
}

#[derive(Args, Debug)]
pub struct AssimilateProjectArgs {
    /// Project path (omit to use current directory)
    #[arg(long)]
    project: Option<PathBuf>,
    /// Evidence projection mode
    #[arg(long = "evidence-mode", value_enum, default_value = "shared-safe")]
    evidence_mode: DreamEvidenceModeArg,
    /// Permit raw local evidence for local-only experiments
    #[arg(long)]
    allow_raw_evidence: bool,
}

#[derive(Args, Debug)]
pub struct AssimilateSourceArgs {
    /// Evidence projection mode
    #[arg(long = "evidence-mode", value_enum, default_value = "shared-safe")]
    evidence_mode: DreamEvidenceModeArg,
    /// Permit raw local evidence for local-only experiments
    #[arg(long)]
    allow_raw_evidence: bool,
    /// Maximum events retained per project before projection
    #[arg(long, default_value_t = 200)]
    per_project_limit: usize,
    /// Keep only events at or after this RFC3339 timestamp
    #[arg(long)]
    since: Option<String>,
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
    after_help = "Status JSON includes store.db_path, store.schema_version, store.expected_schema_version, store.schema_status, remote state, project link state, and diagnostics for sources, privacy filtering, schema, sync, continuity brief provider setup, and assimilation handoff readiness."
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
    /// Hidden JSON-over-stdin project list endpoint
    #[command(name = "list-projects", hide = true)]
    ListProjects(PeerRequestArgs),
    /// Hidden JSON-over-stdin session list endpoint
    #[command(name = "list-sessions", hide = true)]
    ListSessions(PeerRequestArgs),
    /// Hidden JSON-over-stdin session read endpoint
    #[command(name = "read-session", hide = true)]
    ReadSession(PeerRequestArgs),
    /// Hidden JSON-over-stdin project read endpoint
    #[command(name = "read-project", hide = true)]
    ReadProject(PeerRequestArgs),
    /// Hidden JSON-over-stdin source read endpoint
    #[command(name = "read-source", hide = true)]
    ReadSource(PeerRequestArgs),
    /// Hidden JSON-over-stdin project context endpoint
    #[command(name = "context-project", hide = true)]
    ContextProject(PeerRequestArgs),
    /// Hidden JSON-over-stdin source context endpoint
    #[command(name = "context-source", hide = true)]
    ContextSource(PeerRequestArgs),
    /// Hidden JSON-over-stdin recall endpoint
    #[command(name = "recall", hide = true)]
    Recall(PeerRequestArgs),
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
#[command(
    after_help = "Behavior:\n  `mmr assimilate project` returns a system prompt, runbook, output contract, and cited evidence bundle for the calling AI agent. It does not run a provider or write learned memory."
)]
pub struct DreamArgs {
    /// Project path (omit to use current directory)
    #[arg(long)]
    project: Option<PathBuf>,
    /// Evidence projection mode
    #[arg(long = "evidence-mode", value_enum, default_value = "shared-safe")]
    evidence_mode: DreamEvidenceModeArg,
    /// Permit raw local evidence for local-only mock experiments
    #[arg(long)]
    allow_raw_evidence: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
pub enum DreamEvidenceModeArg {
    #[default]
    SharedSafe,
    LocalRaw,
}

impl From<DreamEvidenceModeArg> for DreamEvidenceMode {
    fn from(value: DreamEvidenceModeArg) -> Self {
        match value {
            DreamEvidenceModeArg::SharedSafe => DreamEvidenceMode::SharedSafe,
            DreamEvidenceModeArg::LocalRaw => DreamEvidenceMode::LocalRaw,
        }
    }
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
    /// Read/cache the bundle without applying native provider files
    #[arg(long)]
    read_only: bool,
    /// Apply native provider files after pulling; default when --read-only is absent
    #[arg(long)]
    apply: bool,
    /// Replace existing native files when applying
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
    /// Read/cache the bundle without applying native provider files
    #[arg(long)]
    read_only: bool,
    /// Apply native provider files; default when --read-only is absent
    #[arg(long)]
    apply: bool,
    /// Target project path override for apply
    #[arg(long)]
    project: Option<String>,
    /// Replace existing native files when applying
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
    if let Commands::Mcp(args) = &cli.command {
        crate::mcp::run_mcp(args).await?;
        return Ok(String::new());
    }
    if let Commands::Note { text } = &cli.command {
        return serialize(&note_response(text.clone())?, cli.pretty);
    }
    if let Commands::Find(args) = &cli.command {
        return find_output(args, source_filter, cli.pretty);
    }
    if let Commands::Assimilate(args) = &cli.command {
        return serialize(&assimilate_response(args, cli.source)?, cli.pretty);
    }
    if let Commands::Skill(args) = &cli.command {
        return skill_command_response(args, cli.pretty);
    }
    if let Commands::Import(args) = &cli.command {
        return import_command_response(args, source_filter, cli.pretty);
    }
    if let Commands::Ingest(args) = &cli.command {
        return ingest_command_response(args, source_filter, cli.pretty);
    }
    if let Commands::Share(args) = &cli.command {
        return share_command_response(args, source_filter, cli.pretty);
    }
    if let Commands::Init(args) = &cli.command {
        return serialize(&init_response(args, source_filter)?, cli.pretty);
    }
    if let Commands::Redact(args) = &cli.command {
        return serialize(&redact_response(args, source_filter)?, cli.pretty);
    }
    if let Commands::Sync(args) = &cli.command {
        return serialize(&sync_response(args, source_filter)?, cli.pretty);
    }
    if let Commands::Status(args) = &cli.command {
        return serialize(&status_response(args, source_filter)?, cli.pretty);
    }
    if let Commands::DbInfo {
        project,
        smoke_event,
    } = &cli.command
    {
        return serialize(
            &db_info_response(project.clone(), *smoke_event)?,
            cli.pretty,
        );
    }
    if let Commands::Peer(args) = &cli.command {
        return peer_command_response(args, source_filter, cli.pretty);
    }

    let service = QueryService::load()?;

    let response = match cli.command {
        Commands::List(args) => list_command_response(&service, args, source_filter, cli.pretty)?,
        Commands::Retrieve(args) => {
            retrieve_output(&args, &service, cli.source, source_filter, cli.pretty)?
        }
        Commands::Recall(args) => {
            let options = MessageQueryOptions::new(
                Some(args.limit),
                args.offset,
                SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
            );
            if args.remotes.is_empty() {
                run_session_axis(
                    &service,
                    cli.source,
                    source_filter,
                    cli.pretty,
                    args.project,
                    args.all,
                    SessionAxis::Back(args.n),
                    args.include_newest,
                    options,
                )?
            } else {
                recall_with_remotes_response(
                    &service,
                    args,
                    cli.source,
                    source_filter,
                    cli.pretty,
                    options,
                )?
            }
        }
        Commands::Read(args) => {
            read_command_response(&service, args, cli.source, source_filter, cli.pretty)?
        }
        Commands::Context(args) => {
            context_command_response(&service, args, cli.source, source_filter, cli.pretty)?
        }
        Commands::Summarize(args) => {
            summarize_command_response(&service, args, cli.source, source_filter, cli.pretty)
                .await?
        }
        Commands::Compact(args) => {
            compact_command_response(&service, args, cli.source, source_filter, cli.pretty).await?
        }
        Commands::Import(_) => unreachable!("import handled before QueryService load"),
        Commands::Ingest(_) => unreachable!("ingest handled before QueryService load"),
        Commands::Share(_) => unreachable!("share handled before QueryService load"),
        Commands::Note { text } => serialize(&note_response(text)?, cli.pretty)?,
        Commands::Find(args) => find_output(&args, source_filter, cli.pretty)?,
        Commands::Assimilate(args) => {
            serialize(&assimilate_response(&args, cli.source)?, cli.pretty)?
        }
        Commands::Skill(args) => skill_command_response(&args, cli.pretty)?,
        Commands::Init(args) => serialize(&init_response(&args, source_filter)?, cli.pretty)?,
        Commands::Redact(args) => serialize(&redact_response(&args, source_filter)?, cli.pretty)?,
        Commands::Sync(args) => serialize(&sync_response(&args, source_filter)?, cli.pretty)?,
        Commands::Status(args) => serialize(&status_response(&args, source_filter)?, cli.pretty)?,
        Commands::Mcp(_) => unreachable!("mcp handled before QueryService load"),
        Commands::DbInfo {
            project,
            smoke_event,
        } => serialize(&db_info_response(project, smoke_event)?, cli.pretty)?,
        Commands::Peer(_) => unreachable!("peer handled before QueryService load"),
    };

    Ok(response)
}

fn list_command_response(
    service: &QueryService,
    args: ListArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        ListCommand::Projects(args) => {
            if args.remotes.is_empty() {
                serialize(
                    &service.projects(
                        source_filter,
                        Some(args.limit),
                        args.offset,
                        SortOptions::new(args.sort_by, args.order),
                    ),
                    pretty,
                )
            } else {
                list_projects_with_remotes_response(service, args, source_filter, pretty)
            }
        }
        ListCommand::Sessions(args) => {
            if args.remotes.is_empty() {
                serialize(
                    &service.sessions(
                        effective_project_scope(args.project, args.all).as_deref(),
                        source_filter,
                        Some(args.limit),
                        args.offset,
                        SortOptions::new(args.sort_by, args.order),
                    )?,
                    pretty,
                )
            } else {
                list_sessions_with_remotes_response(service, args, source_filter, pretty)
            }
        }
    }
}

fn list_projects_with_remotes_response(
    service: &QueryService,
    args: ListProjectsArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let mut local = service.projects(
        source_filter,
        None,
        0,
        SortOptions::new(args.sort_by, args.order),
    );
    let mut peer_results = Vec::new();
    let request = PeerListProjectsRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        source: source_filter,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
        sort_by: args.sort_by,
        order: args.order,
    };
    for remote in &args.remotes {
        let mut response = peer_list_projects(remote, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&response.peer_results);
        annotate_peer_projects(&mut response.projects, remote, remote_mmr_version.clone());
        peer_results.push(ApiPeerResult {
            host: remote.to_string(),
            transport: "ssh".to_string(),
            command: "list/projects".to_string(),
            status: "ok".to_string(),
            remote_mmr_version,
            total_messages: Some(response.total_messages),
            total_sessions: Some(response.total_sessions),
        });
        local.total_messages += response.total_messages;
        local.total_sessions += response.total_sessions;
        local.projects.extend(response.projects);
    }
    sort_api_projects(&mut local.projects, args.sort_by, args.order);
    local.projects = apply_generic_pagination(local.projects, Some(args.limit), args.offset);
    local.peer_results = Some(peer_results);
    serialize(&local, pretty)
}

fn list_sessions_with_remotes_response(
    service: &QueryService,
    args: ListSessionsArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let project_scope = effective_project_scope(args.project.clone(), args.all);
    let mut local = service.sessions(
        project_scope.as_deref(),
        source_filter,
        None,
        0,
        SortOptions::new(args.sort_by, args.order),
    )?;
    let project_for_identity = project_identity_input(project_scope.as_deref())?;
    let request = PeerListSessionsRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        project: build_peer_project_identity(&project_for_identity),
        source: source_filter,
        all: args.all,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
        sort_by: args.sort_by,
        order: args.order,
    };
    let mut peer_results = Vec::new();
    for remote in &args.remotes {
        let mut response = peer_list_sessions(remote, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&response.peer_results);
        annotate_peer_sessions(&mut response.sessions, remote, remote_mmr_version.clone());
        peer_results.push(ApiPeerResult {
            host: remote.to_string(),
            transport: "ssh".to_string(),
            command: "list/sessions".to_string(),
            status: "ok".to_string(),
            remote_mmr_version,
            total_messages: None,
            total_sessions: Some(response.total_sessions),
        });
        local.sessions.extend(response.sessions);
    }
    sort_api_sessions(&mut local.sessions, args.sort_by, args.order);
    local.total_sessions = local.sessions.len() as i64;
    local.sessions = apply_generic_pagination(local.sessions, Some(args.limit), args.offset);
    local.peer_results = Some(peer_results);
    serialize(&local, pretty)
}

fn read_command_response(
    service: &QueryService,
    args: ReadArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        ReadCommand::Session(args) => read_session_response(service, args, source_filter, pretty),
        ReadCommand::Project(args) => read_project_response(service, args, source_filter, pretty),
        ReadCommand::Source(args) => {
            let source = require_explicit_source(cli_source, "read source")?;
            read_source_response(service, args, source, pretty)
        }
    }
}

fn read_session_response(
    service: &QueryService,
    args: ReadSessionArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    if !args.remotes.is_empty() {
        return read_session_with_remotes_response(service, args, source_filter, pretty);
    }

    if args.format == ReadFormatArg::Tree {
        let store = Store::open_default()?;
        let events =
            store.events_for_source_session(&args.session_id, source_filter_name(source_filter))?;
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
        MessageQueryOptions::new(
            args.limit,
            args.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    serialize(&response, pretty)
}

fn read_project_response(
    service: &QueryService,
    args: ReadProjectArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    if !args.remotes.is_empty() {
        return read_project_with_remotes_response(service, args, source_filter, pretty);
    }

    if args.format == ReadFormatArg::Tree {
        let response = export_tree_project_response(args.project, args.output_dir, source_filter)?;
        return serialize(&response, pretty);
    }

    let sort = SortOptions::new(SortBy::Timestamp, SortOrder::Asc);
    if let Some(project) = args.project {
        let response = service.messages(
            &[],
            Some(project.as_str()),
            source_filter,
            MessageQueryOptions::new(args.limit, args.offset, sort),
        )?;
        let mut response = response;
        if response.next_page {
            response.next_command = Some(build_next_read_project_command(
                source_filter,
                Some(project.as_str()),
                args.limit,
                response.next_offset as usize,
            ));
        }
        return serialize(&response, pretty);
    }

    let mut response = read_cwd_project_messages(service, source_filter, args.limit, args.offset)?;
    if response.next_page {
        response.next_command = Some(build_next_read_project_command(
            source_filter,
            None,
            args.limit,
            response.next_offset as usize,
        ));
    }
    serialize(&response, pretty)
}

fn read_source_response(
    service: &QueryService,
    args: ReadSourceArgs,
    source: SourceFilter,
    pretty: bool,
) -> Result<String> {
    if !args.remotes.is_empty() {
        return read_source_with_remotes_response(service, args, source, pretty);
    }

    if args.format == ReadFormatArg::Tree {
        let store = Store::open_default()?;
        let events = store.events_for_source(source_name(source), None, None)?;
        let response = export_tree_events_response(events, args.output_dir, "source")?;
        return serialize(&response, pretty);
    }

    let response = service.messages(
        &[],
        None,
        Some(source),
        MessageQueryOptions::new(
            args.limit,
            args.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    serialize(&response, pretty)
}

fn read_cwd_project_messages(
    service: &QueryService,
    source_filter: Option<SourceFilter>,
    limit: Option<usize>,
    offset: usize,
) -> Result<ApiMessagesResponse> {
    let sort = SortOptions::new(SortBy::Timestamp, SortOrder::Asc);
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
                MessageQueryOptions::new(None, 0, sort),
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
    let paged = apply_message_output_pagination(messages, limit, offset);
    let next_offset = offset.saturating_add(paged.len()) as i64;
    Ok(ApiMessagesResponse {
        messages: paged,
        total_messages: total_messages.max(total as i64),
        next_page: next_offset < total as i64,
        next_offset,
        next_command: None,
        session_selection: None,
        peer_results: None,
    })
}

fn read_project_with_remotes_response(
    service: &QueryService,
    args: ReadProjectArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    if args.format == ReadFormatArg::Tree {
        return Err(anyhow::Error::new(peer_cli_failure(
            "read/project",
            "peer_unsupported_format",
            None,
            "--remote is not supported with --format tree",
            pretty,
        )?));
    }

    let project_for_identity = project_identity_input(args.project.as_deref())?;
    let request = PeerProjectRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        project: build_peer_project_identity(&project_for_identity),
        source: source_filter,
        all: false,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
        recall: None,
    };

    let mut local =
        local_project_messages_unpaged(service, args.project.as_deref(), source_filter)?;
    let mut peer_results = Vec::new();
    for remote_name in &args.remotes {
        let mut remote = peer_read_project(remote_name, &request, pretty)?;
        let remote_mmr_version = remote
            .peer_results
            .as_ref()
            .and_then(|items| items.first())
            .and_then(|item| item.remote_mmr_version.clone());
        annotate_peer_messages(
            &mut remote.messages,
            remote_name,
            remote_mmr_version.clone(),
        );
        peer_results.push(peer_result_for_messages(
            remote_name,
            "read/project",
            remote.total_messages,
            remote_mmr_version,
        ));
        local.messages.extend(remote.messages);
    }

    let mut messages = dedup_api_messages(local.messages);
    sort_api_messages_chronological(&mut messages);
    let total = messages.len() as i64;
    let paged = apply_message_output_pagination(messages, args.limit, args.offset);
    let next_offset = args.offset.saturating_add(paged.len()) as i64;
    let next_page = args.limit.is_some() && next_offset < total;

    let mut response = ApiMessagesResponse {
        messages: paged,
        total_messages: total,
        next_page,
        next_offset,
        next_command: None,
        session_selection: None,
        peer_results: Some(peer_results),
    };
    if response.next_page {
        response.next_command = Some(build_next_read_project_command_with_remotes(
            source_filter,
            args.project.as_deref(),
            &args.remotes,
            args.limit,
            response.next_offset as usize,
        ));
    }
    serialize(&response, pretty)
}

fn local_project_messages_unpaged(
    service: &QueryService,
    project: Option<&str>,
    source_filter: Option<SourceFilter>,
) -> Result<ApiMessagesResponse> {
    let sort = SortOptions::new(SortBy::Timestamp, SortOrder::Asc);
    if let Some(project) = project {
        service.messages(
            &[],
            Some(project),
            source_filter,
            MessageQueryOptions::new(None, 0, sort),
        )
    } else {
        read_cwd_project_messages(service, source_filter, None, 0)
    }
}

fn read_session_with_remotes_response(
    service: &QueryService,
    args: ReadSessionArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    if args.format == ReadFormatArg::Tree {
        return Err(anyhow::Error::new(peer_cli_failure(
            "read/session",
            "peer_unsupported_format",
            None,
            "--remote is not supported with --format tree",
            pretty,
        )?));
    }

    let mut local = service.messages(
        std::slice::from_ref(&args.session_id),
        args.project.as_deref(),
        source_filter,
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let project_for_identity = project_identity_input(args.project.as_deref())?;
    let request = PeerReadSessionRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        session_id: args.session_id.clone(),
        project: build_peer_project_identity(&project_for_identity),
        source: source_filter,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
    };

    let mut peer_results = Vec::new();
    for remote_name in &args.remotes {
        let mut remote = peer_read_session(remote_name, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&remote.peer_results);
        annotate_peer_messages(
            &mut remote.messages,
            remote_name,
            remote_mmr_version.clone(),
        );
        peer_results.push(peer_result_for_messages(
            remote_name,
            "read/session",
            remote.total_messages,
            remote_mmr_version,
        ));
        local.messages.extend(remote.messages);
    }

    let mut messages = dedup_api_messages(local.messages);
    sort_api_messages_chronological(&mut messages);
    let total = messages.len() as i64;
    let paged = apply_message_output_pagination(messages, args.limit, args.offset);
    let next_offset = args.offset.saturating_add(paged.len()) as i64;
    let next_page = args.limit.is_some() && next_offset < total;
    let response = ApiMessagesResponse {
        messages: paged,
        total_messages: total,
        next_page,
        next_offset,
        next_command: None,
        session_selection: None,
        peer_results: Some(peer_results),
    };
    serialize(&response, pretty)
}

fn read_source_with_remotes_response(
    service: &QueryService,
    args: ReadSourceArgs,
    source: SourceFilter,
    pretty: bool,
) -> Result<String> {
    if args.format == ReadFormatArg::Tree {
        return Err(anyhow::Error::new(peer_cli_failure(
            "read/source",
            "peer_unsupported_format",
            None,
            "--remote is not supported with --format tree",
            pretty,
        )?));
    }

    let mut local = service.messages(
        &[],
        None,
        Some(source),
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let request = PeerReadSourceRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        source,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
    };
    let mut peer_results = Vec::new();
    for remote_name in &args.remotes {
        let mut remote = peer_read_source(remote_name, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&remote.peer_results);
        annotate_peer_messages(
            &mut remote.messages,
            remote_name,
            remote_mmr_version.clone(),
        );
        peer_results.push(peer_result_for_messages(
            remote_name,
            "read/source",
            remote.total_messages,
            remote_mmr_version,
        ));
        local.messages.extend(remote.messages);
    }
    let mut messages = dedup_api_messages(local.messages);
    sort_api_messages_chronological(&mut messages);
    let total = messages.len() as i64;
    let paged = apply_message_output_pagination(messages, args.limit, args.offset);
    let next_offset = args.offset.saturating_add(paged.len()) as i64;
    let next_page = args.limit.is_some() && next_offset < total;
    let response = ApiMessagesResponse {
        messages: paged,
        total_messages: total,
        next_page,
        next_offset,
        next_command: None,
        session_selection: None,
        peer_results: Some(peer_results),
    };
    serialize(&response, pretty)
}

fn peer_read_project(
    host: &str,
    request: &PeerProjectRequest,
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "read-project", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("read/project", error, pretty))
}

fn peer_list_projects(
    host: &str,
    request: &PeerListProjectsRequest,
    pretty: bool,
) -> Result<ApiProjectsResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "list-projects", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("list/projects", error, pretty))
}

fn peer_list_sessions(
    host: &str,
    request: &PeerListSessionsRequest,
    pretty: bool,
) -> Result<ApiSessionsResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "list-sessions", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("list/sessions", error, pretty))
}

fn peer_read_session(
    host: &str,
    request: &PeerReadSessionRequest,
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "read-session", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("read/session", error, pretty))
}

fn peer_read_source(
    host: &str,
    request: &PeerReadSourceRequest,
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "read-source", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("read/source", error, pretty))
}

fn peer_context_project(
    host: &str,
    request: &PeerProjectRequest,
    pretty: bool,
) -> Result<ContextResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "context-project", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("context/project", error, pretty))
}

fn peer_context_source(
    host: &str,
    request: &PeerReadSourceRequest,
    pretty: bool,
) -> Result<ContextResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "context-source", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("context/source", error, pretty))
}

fn peer_recall(
    host: &str,
    request: &PeerProjectRequest,
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    run_peer_json(
        host,
        &["mmr", "peer", "recall", "--request-json", "-"],
        Some(request),
    )
    .map_err(|error| peer_anyhow_error("recall", error, pretty))
}

fn annotate_peer_messages(
    messages: &mut [ApiMessage],
    host: &str,
    remote_mmr_version: Option<String>,
) {
    for message in messages {
        message.origin = Some(ApiMessageOrigin {
            host: host.to_string(),
            transport: "ssh".to_string(),
            remote_mmr_version: remote_mmr_version.clone(),
        });
    }
}

fn annotate_peer_projects(
    projects: &mut [ApiProject],
    host: &str,
    remote_mmr_version: Option<String>,
) {
    for project in projects {
        project.origin = Some(ApiMessageOrigin {
            host: host.to_string(),
            transport: "ssh".to_string(),
            remote_mmr_version: remote_mmr_version.clone(),
        });
    }
}

fn annotate_peer_sessions(
    sessions: &mut [ApiSession],
    host: &str,
    remote_mmr_version: Option<String>,
) {
    for session in sessions {
        session.origin = Some(ApiMessageOrigin {
            host: host.to_string(),
            transport: "ssh".to_string(),
            remote_mmr_version: remote_mmr_version.clone(),
        });
    }
}

fn remote_version_from_peer_results(peer_results: &Option<Vec<ApiPeerResult>>) -> Option<String> {
    peer_results
        .as_ref()
        .and_then(|items| items.first())
        .and_then(|item| item.remote_mmr_version.clone())
}

fn peer_result_for_messages(
    host: &str,
    command: &str,
    total_messages: i64,
    remote_mmr_version: Option<String>,
) -> ApiPeerResult {
    ApiPeerResult {
        host: host.to_string(),
        transport: "ssh".to_string(),
        command: command.to_string(),
        status: "ok".to_string(),
        remote_mmr_version,
        total_messages: Some(total_messages),
        total_sessions: None,
    }
}

fn apply_generic_pagination<T>(items: Vec<T>, limit: Option<usize>, offset: usize) -> Vec<T> {
    let iter = items.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

fn sort_api_projects(projects: &mut [ApiProject], sort_by: SortBy, order: SortOrder) {
    projects.sort_by(|a, b| {
        let ordering = match sort_by {
            SortBy::Timestamp => a
                .last_activity
                .cmp(&b.last_activity)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.source.cmp(&b.source)),
            SortBy::MessageCount => a
                .message_count
                .cmp(&b.message_count)
                .then_with(|| a.last_activity.cmp(&b.last_activity))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.source.cmp(&b.source)),
        };
        match order {
            SortOrder::Asc => ordering,
            SortOrder::Desc => ordering.reverse(),
        }
    });
}

fn sort_api_sessions(sessions: &mut [ApiSession], sort_by: SortBy, order: SortOrder) {
    sessions.sort_by(|a, b| {
        let ordering = match sort_by {
            SortBy::Timestamp => a
                .last_timestamp
                .cmp(&b.last_timestamp)
                .then_with(|| a.session_id.cmp(&b.session_id))
                .then_with(|| a.source.cmp(&b.source)),
            SortBy::MessageCount => a
                .message_count
                .cmp(&b.message_count)
                .then_with(|| a.last_timestamp.cmp(&b.last_timestamp))
                .then_with(|| a.session_id.cmp(&b.session_id))
                .then_with(|| a.source.cmp(&b.source)),
        };
        match order {
            SortOrder::Asc => ordering,
            SortOrder::Desc => ordering.reverse(),
        }
    });
}

fn dedup_api_messages(messages: Vec<ApiMessage>) -> Vec<ApiMessage> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for message in messages {
        let key = (
            message.source.clone(),
            message.project_name.clone(),
            message.session_id.clone(),
            message.timestamp.clone(),
            message.role.clone(),
            message.content.clone(),
        );
        if seen.insert(key) {
            deduped.push(message);
        }
    }
    deduped
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

fn newest_window_api_messages(mut messages: Vec<ApiMessage>, limit: usize) -> Vec<ApiMessage> {
    sort_api_messages_chronological(&mut messages);
    let mut window = messages.into_iter().rev().take(limit).collect::<Vec<_>>();
    window.reverse();
    window
}

fn apply_message_output_pagination(
    messages: Vec<ApiMessage>,
    limit: Option<usize>,
    offset: usize,
) -> Vec<ApiMessage> {
    let iter = messages.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

fn apply_message_page_to_response(
    mut response: ApiMessagesResponse,
    limit: Option<usize>,
    offset: usize,
) -> ApiMessagesResponse {
    let total = response.messages.len() as i64;
    if limit.is_none() && offset == 0 {
        response.total_messages = total;
        return response;
    }
    sort_api_messages_chronological(&mut response.messages);
    let descending = response.messages.into_iter().rev().collect::<Vec<_>>();
    let mut paged = apply_message_output_pagination(descending, limit, offset);
    paged.reverse();
    response.messages = paged;
    response.total_messages = total;
    response
}

fn build_next_read_project_command(
    source_filter: Option<SourceFilter>,
    project: Option<&str>,
    limit: Option<usize>,
    next_offset: usize,
) -> String {
    let mut parts = vec!["mmr".to_string()];
    if let Some(source) = source_filter {
        parts.push(format!("--source {}", source_name(source)));
    }
    parts.push("read project".to_string());
    if let Some(project) = project {
        parts.push(format!("--project {project}"));
    }
    if let Some(limit) = limit {
        parts.push(format!("--limit {limit}"));
    }
    parts.push(format!("--offset {next_offset}"));
    parts.join(" ")
}

fn build_next_read_project_command_with_remotes(
    source_filter: Option<SourceFilter>,
    project: Option<&str>,
    remotes: &[String],
    limit: Option<usize>,
    next_offset: usize,
) -> String {
    let mut parts = vec!["mmr".to_string()];
    if let Some(source) = source_filter {
        parts.push(format!("--source {}", source_name(source)));
    }
    parts.push("read project".to_string());
    if let Some(project) = project {
        parts.push(format!("--project {project}"));
    }
    for remote in remotes {
        parts.push(format!("--remote {remote}"));
    }
    if let Some(limit) = limit {
        parts.push(format!("--limit {limit}"));
    }
    parts.push(format!("--offset {next_offset}"));
    parts.join(" ")
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextResponse {
    command: String,
    scope: String,
    source: Option<String>,
    project: Option<String>,
    total_sessions: i64,
    total_messages: i64,
    sessions: Vec<crate::types::ApiSession>,
    messages: Vec<ApiMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    peer_results: Option<Vec<ApiPeerResult>>,
}

fn context_command_response(
    service: &QueryService,
    args: ContextArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        ContextCommand::Project(args) => {
            if !args.remotes.is_empty() {
                return context_project_with_remotes_response(service, args, source_filter, pretty);
            }
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
                MessageQueryOptions::new(
                    Some(args.limit),
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )?;
            serialize(
                &ContextResponse {
                    command: "context/project".to_string(),
                    scope: "project".to_string(),
                    source: source_filter.map(source_name).map(str::to_string),
                    project,
                    total_sessions: sessions.total_sessions,
                    total_messages: messages.total_messages,
                    sessions: sessions.sessions,
                    messages: messages.messages,
                    peer_results: None,
                },
                pretty,
            )
        }
        ContextCommand::Source(args) => {
            let source = require_explicit_source(cli_source, "context source")?;
            if !args.remotes.is_empty() {
                return context_source_with_remotes_response(service, args, source, pretty);
            }
            let sessions = service.sessions(
                None,
                Some(source),
                None,
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
            )?;
            let messages = service.messages(
                &[],
                None,
                Some(source),
                MessageQueryOptions::new(
                    Some(args.limit),
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )?;
            serialize(
                &ContextResponse {
                    command: "context/source".to_string(),
                    scope: "source".to_string(),
                    source: Some(source_name(source).to_string()),
                    project: None,
                    total_sessions: sessions.total_sessions,
                    total_messages: messages.total_messages,
                    sessions: sessions.sessions,
                    messages: messages.messages,
                    peer_results: None,
                },
                pretty,
            )
        }
    }
}

fn context_project_with_remotes_response(
    service: &QueryService,
    args: ContextProjectArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let project = args
        .project
        .clone()
        .or_else(|| effective_project_scope(None, false));
    let project_for_identity = project_identity_input(project.as_deref())?;
    let request = PeerProjectRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        project: build_peer_project_identity(&project_for_identity),
        source: source_filter,
        all: false,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
        recall: None,
    };

    let sessions = service.sessions(
        project.as_deref(),
        source_filter,
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    )?;
    let local_messages = service.messages(
        &[],
        project.as_deref(),
        source_filter,
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let local_total_messages = local_messages.total_messages;

    let mut all_sessions = sessions.sessions;
    let mut all_messages = local_messages.messages;
    let mut peer_results = Vec::new();

    for remote_name in &args.remotes {
        let mut remote = peer_context_project(remote_name, &request, pretty)?;
        let remote_mmr_version = remote
            .peer_results
            .as_ref()
            .and_then(|items| items.first())
            .and_then(|item| item.remote_mmr_version.clone());
        annotate_peer_messages(
            &mut remote.messages,
            remote_name,
            remote_mmr_version.clone(),
        );
        annotate_peer_sessions(
            &mut remote.sessions,
            remote_name,
            remote_mmr_version.clone(),
        );
        peer_results.push(ApiPeerResult {
            host: remote_name.to_string(),
            transport: "ssh".to_string(),
            command: "context/project".to_string(),
            status: "ok".to_string(),
            remote_mmr_version,
            total_messages: Some(remote.total_messages),
            total_sessions: Some(remote.total_sessions),
        });
        all_sessions.extend(remote.sessions);
        all_messages.extend(remote.messages);
    }

    all_messages = dedup_api_messages(all_messages);
    let messages = newest_window_api_messages(all_messages, args.limit);
    all_sessions.sort_by(|a, b| {
        b.last_timestamp
            .cmp(&a.last_timestamp)
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.source.cmp(&b.source))
    });
    all_sessions.dedup_by(|a, b| {
        a.source == b.source && a.project_name == b.project_name && a.session_id == b.session_id
    });

    serialize(
        &ContextResponse {
            command: "context/project".to_string(),
            scope: "project".to_string(),
            source: source_filter.map(source_name).map(str::to_string),
            project,
            total_sessions: all_sessions.len() as i64,
            total_messages: local_total_messages
                + peer_results
                    .iter()
                    .filter_map(|result| result.total_messages)
                    .sum::<i64>(),
            sessions: all_sessions,
            messages,
            peer_results: Some(peer_results),
        },
        pretty,
    )
}

fn context_source_with_remotes_response(
    service: &QueryService,
    args: ContextSourceArgs,
    source: SourceFilter,
    pretty: bool,
) -> Result<String> {
    let sessions = service.sessions(
        None,
        Some(source),
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    )?;
    let local_messages = service.messages(
        &[],
        None,
        Some(source),
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let local_total_messages = local_messages.total_messages;
    let request = PeerReadSourceRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        source,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
    };

    let mut all_sessions = sessions.sessions;
    let mut all_messages = local_messages.messages;
    let mut peer_results = Vec::new();
    for remote_name in &args.remotes {
        let mut remote = peer_context_source(remote_name, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&remote.peer_results);
        annotate_peer_messages(
            &mut remote.messages,
            remote_name,
            remote_mmr_version.clone(),
        );
        annotate_peer_sessions(
            &mut remote.sessions,
            remote_name,
            remote_mmr_version.clone(),
        );
        peer_results.push(ApiPeerResult {
            host: remote_name.to_string(),
            transport: "ssh".to_string(),
            command: "context/source".to_string(),
            status: "ok".to_string(),
            remote_mmr_version,
            total_messages: Some(remote.total_messages),
            total_sessions: Some(remote.total_sessions),
        });
        all_sessions.extend(remote.sessions);
        all_messages.extend(remote.messages);
    }

    all_messages = dedup_api_messages(all_messages);
    let messages = newest_window_api_messages(all_messages, args.limit);
    sort_api_sessions(&mut all_sessions, SortBy::Timestamp, SortOrder::Desc);

    serialize(
        &ContextResponse {
            command: "context/source".to_string(),
            scope: "source".to_string(),
            source: Some(source_name(source).to_string()),
            project: None,
            total_sessions: all_sessions.len() as i64,
            total_messages: local_total_messages
                + peer_results
                    .iter()
                    .filter_map(|result| result.total_messages)
                    .sum::<i64>(),
            sessions: all_sessions,
            messages,
            peer_results: Some(peer_results),
        },
        pretty,
    )
}

fn project_messages_with_remotes_for_summary(
    service: &QueryService,
    project: Option<&str>,
    source_filter: Option<SourceFilter>,
    remotes: &[String],
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    let mut local = local_project_messages_unpaged(service, project, source_filter)?;
    let project_for_identity = project_identity_input(project)?;
    let request = PeerProjectRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        project: build_peer_project_identity(&project_for_identity),
        source: source_filter,
        all: false,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
        recall: None,
    };
    for remote_name in remotes {
        let mut remote = peer_read_project(remote_name, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&remote.peer_results);
        annotate_peer_messages(&mut remote.messages, remote_name, remote_mmr_version);
        local.messages.extend(remote.messages);
    }
    local.messages = dedup_api_messages(local.messages);
    sort_api_messages_chronological(&mut local.messages);
    local.total_messages = local.messages.len() as i64;
    Ok(local)
}

fn session_messages_with_remotes_for_summary(
    service: &QueryService,
    session_id: &str,
    project: Option<&str>,
    source_filter: Option<SourceFilter>,
    remotes: &[String],
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    let mut local = service.messages(
        &[session_id.to_string()],
        project,
        source_filter,
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let project_for_identity = project_identity_input(project)?;
    let request = PeerReadSessionRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        session_id: session_id.to_string(),
        project: build_peer_project_identity(&project_for_identity),
        source: source_filter,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
    };
    for remote_name in remotes {
        let mut remote = peer_read_session(remote_name, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&remote.peer_results);
        annotate_peer_messages(&mut remote.messages, remote_name, remote_mmr_version);
        local.messages.extend(remote.messages);
    }
    local.messages = dedup_api_messages(local.messages);
    sort_api_messages_chronological(&mut local.messages);
    local.total_messages = local.messages.len() as i64;
    Ok(local)
}

fn source_messages_with_remotes_for_summary(
    service: &QueryService,
    source: SourceFilter,
    remotes: &[String],
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    let mut local = service.messages(
        &[],
        None,
        Some(source),
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let request = PeerReadSourceRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        source,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
    };
    for remote_name in remotes {
        let mut remote = peer_read_source(remote_name, &request, pretty)?;
        let remote_mmr_version = remote_version_from_peer_results(&remote.peer_results);
        annotate_peer_messages(&mut remote.messages, remote_name, remote_mmr_version);
        local.messages.extend(remote.messages);
    }
    local.messages = dedup_api_messages(local.messages);
    sort_api_messages_chronological(&mut local.messages);
    local.total_messages = local.messages.len() as i64;
    Ok(local)
}

async fn summarize_command_response(
    service: &QueryService,
    args: SummarizeArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        SummarizeCommand::Project(args) => {
            let project = args
                .project
                .unwrap_or(current_dir_project().context("could not resolve current directory")?);
            let model = effective_summariser_model(args.runner.model.as_deref());
            let page = args.limit.is_some() || args.offset > 0;
            if !args.remotes.is_empty() || page {
                let messages = if !args.remotes.is_empty() {
                    project_messages_with_remotes_for_summary(
                        service,
                        Some(project.as_str()),
                        source_filter,
                        &args.remotes,
                        pretty,
                    )?
                } else {
                    local_project_messages_unpaged(service, Some(project.as_str()), source_filter)?
                };
                let messages = apply_message_page_to_response(messages, args.limit, args.offset);
                let formatted = format_messages_as_transcript_input(&messages.messages);
                let response = ai::summarize_formatted_messages(
                    args.runner.instructions.as_deref(),
                    &model,
                    &formatted,
                )
                .await?;
                return format_remember_response(&response, args.runner.output_format, pretty);
            }
            let response = ai::remember(
                service,
                RememberRequest {
                    project: project.as_str(),
                    selection: RememberSelection::All,
                    source: source_filter,
                    instructions: args.runner.instructions.as_deref(),
                    model: &model,
                },
            )
            .await?;
            format_remember_response(&response, args.runner.output_format, pretty)
        }
        SummarizeCommand::Session(args) => {
            let messages = if args.remotes.is_empty() {
                service.messages(
                    std::slice::from_ref(&args.session_id),
                    args.project.as_deref(),
                    source_filter,
                    MessageQueryOptions::new(
                        None,
                        0,
                        SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                    ),
                )?
            } else {
                session_messages_with_remotes_for_summary(
                    service,
                    &args.session_id,
                    args.project.as_deref(),
                    source_filter,
                    &args.remotes,
                    pretty,
                )?
            };
            let messages = apply_message_page_to_response(messages, args.limit, args.offset);
            let formatted = format_messages_as_transcript_input(&messages.messages);
            let model = effective_summariser_model(args.runner.model.as_deref());
            let response = ai::summarize_formatted_messages(
                args.runner.instructions.as_deref(),
                &model,
                &formatted,
            )
            .await?;
            format_remember_response(&response, args.runner.output_format, pretty)
        }
        SummarizeCommand::Source(args) => {
            let source = require_explicit_source(cli_source, "summarize source")?;
            let messages = if args.remotes.is_empty() {
                service.messages(
                    &[],
                    None,
                    Some(source),
                    MessageQueryOptions::new(
                        None,
                        0,
                        SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                    ),
                )?
            } else {
                source_messages_with_remotes_for_summary(service, source, &args.remotes, pretty)?
            };
            let formatted = format_messages_as_transcript_input(&messages.messages);
            let model = effective_summariser_model(args.runner.model.as_deref());
            let response = ai::summarize_formatted_messages(
                args.runner.instructions.as_deref(),
                &model,
                &formatted,
            )
            .await?;
            format_remember_response(&response, args.runner.output_format, pretty)
        }
    }
}

async fn compact_command_response(
    service: &QueryService,
    args: CompactArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        CompactCommand::Project(args) => {
            let project = args
                .project
                .unwrap_or(current_dir_project().context("could not resolve current directory")?);
            let messages = if args.remotes.is_empty() {
                service.messages(
                    &[],
                    Some(project.as_str()),
                    source_filter,
                    MessageQueryOptions::new(
                        None,
                        0,
                        SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                    ),
                )?
            } else {
                project_messages_with_remotes_for_summary(
                    service,
                    Some(project.as_str()),
                    source_filter,
                    &args.remotes,
                    pretty,
                )?
            };
            let formatted = format_messages_as_transcript_input(&messages.messages);
            let response = compact_formatted_messages(&args.runner, &formatted).await?;
            format_compact_response(&response, args.runner.output_format, pretty)
        }
        CompactCommand::Session(args) => {
            let messages = if args.remotes.is_empty() {
                service.messages(
                    std::slice::from_ref(&args.session_id),
                    args.project.as_deref(),
                    source_filter,
                    MessageQueryOptions::new(
                        None,
                        0,
                        SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                    ),
                )?
            } else {
                session_messages_with_remotes_for_summary(
                    service,
                    &args.session_id,
                    args.project.as_deref(),
                    source_filter,
                    &args.remotes,
                    pretty,
                )?
            };
            let formatted = format_messages_as_transcript_input(&messages.messages);
            let response = compact_formatted_messages(&args.runner, &formatted).await?;
            format_compact_response(&response, args.runner.output_format, pretty)
        }
        CompactCommand::Source(args) => {
            let source = require_explicit_source(cli_source, "compact source")?;
            let messages = if args.remotes.is_empty() {
                service.messages(
                    &[],
                    None,
                    Some(source),
                    MessageQueryOptions::new(
                        None,
                        0,
                        SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                    ),
                )?
            } else {
                source_messages_with_remotes_for_summary(service, source, &args.remotes, pretty)?
            };
            let formatted = format_messages_as_transcript_input(&messages.messages);
            let response = compact_formatted_messages(&args.runner, &formatted).await?;
            format_compact_response(&response, args.runner.output_format, pretty)
        }
    }
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
        ImportCommand::Session(args) => {
            match import_session_response(args, source_filter, pretty) {
                Ok(response) => serialize(&response, pretty),
                Err(failure) => {
                    teleport_fail(rebrand_teleport_failure(failure, "import/session"), pretty)
                }
            }
        }
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
    let read_only = import_read_only_mode(args.read_only, args.apply).map_err(|failure| {
        anyhow::Error::new(
            CliFailure::from_teleport(rebrand_teleport_failure(failure, "import/bundle"), pretty)
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
) -> std::result::Result<bool, TeleportFailure> {
    if read_only && apply {
        return Err(TeleportFailure::usage(
            "import/bundle",
            "pass either --read-only or --apply, not both",
        ));
    }
    Ok(read_only)
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
    pretty: bool,
) -> std::result::Result<ImportSessionResponse, TeleportFailure> {
    if pull.session.is_some() && pull.latest {
        return Err(TeleportFailure::usage(
            "import/session",
            "pass either --session or --latest, not both",
        ));
    }
    if pull.read_only && pull.apply {
        return Err(TeleportFailure::usage(
            "import/session",
            "pass either --read-only or --apply, not both",
        ));
    }
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
    .map_err(|error| peer_error_to_teleport_failure("import/session", error, pretty))?;

    let bundle_id = remote.bundle_id.clone();
    let bundle_path = teleport_pull_cache_path(&bundle_id).map_err(|error| {
        TeleportFailure::runtime(
            "import/session",
            format!("resolve pull cache path: {error}"),
        )
    })?;
    write_bundle(&bundle_path, &remote.bundle)
        .map_err(|error| TeleportFailure::runtime("import/session", error.to_string()))?;

    let (read, apply, status) = if pull.read_only {
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
        read_only: pull.read_only,
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
    _pretty: bool,
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
        PeerCommand::ListProjects(args) => {
            let request: PeerListProjectsRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_list_projects_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
        PeerCommand::ListSessions(args) => {
            let request: PeerListSessionsRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_list_sessions_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
        PeerCommand::ReadSession(args) => {
            let request: PeerReadSessionRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_read_session_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
        PeerCommand::ReadProject(args) => {
            let request: PeerProjectRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_read_project_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
        PeerCommand::ReadSource(args) => {
            let request: PeerReadSourceRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_read_source_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
        PeerCommand::ContextProject(args) => {
            let request: PeerProjectRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_context_project_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
        PeerCommand::ContextSource(args) => {
            let request: PeerReadSourceRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_context_source_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
        PeerCommand::Recall(args) => {
            let request: PeerProjectRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_recall_local_response(&service, request, source_filter, pretty)?;
            serialize(&response, pretty)
        }
        PeerCommand::TeleportPack(args) => {
            let request: PeerTeleportPackRequest = read_peer_request(&args.request_json)?;
            let service = QueryService::load()?;
            let response = peer_teleport_pack_local_response(&service, request, source_filter)?;
            serialize(&response, pretty)
        }
    }
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

fn peer_list_projects_local_response(
    service: &QueryService,
    request: PeerListProjectsRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<ApiProjectsResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = request.source.or(cli_source_filter);
    let mut response = service.projects(
        source,
        request.limits.limit,
        request.limits.offset,
        SortOptions::new(request.sort_by, request.order),
    );
    response.peer_results = Some(vec![local_peer_result(
        "list/projects",
        Some(response.total_messages),
        Some(response.total_sessions),
    )]);
    Ok(response)
}

fn peer_list_sessions_local_response(
    service: &QueryService,
    request: PeerListSessionsRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<ApiSessionsResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = request.source.or(cli_source_filter);
    let project = if request.all {
        None
    } else {
        resolve_peer_project(service, &request.project, source)?
    };
    let mut response = service.sessions(
        project.as_deref(),
        source,
        request.limits.limit,
        request.limits.offset,
        SortOptions::new(request.sort_by, request.order),
    )?;
    response.peer_results = Some(vec![local_peer_result(
        "list/sessions",
        None,
        Some(response.total_sessions),
    )]);
    Ok(response)
}

fn peer_read_session_local_response(
    service: &QueryService,
    request: PeerReadSessionRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<ApiMessagesResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = request.source.or(cli_source_filter);
    let project = resolve_peer_project(service, &request.project, source).unwrap_or(None);
    let mut response = service.messages(
        &[request.session_id],
        project.as_deref(),
        source,
        MessageQueryOptions::new(
            request.limits.limit,
            request.limits.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    response.peer_results = Some(vec![local_peer_result(
        "read/session",
        Some(response.total_messages),
        None,
    )]);
    Ok(response)
}

fn peer_read_source_local_response(
    service: &QueryService,
    request: PeerReadSourceRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<ApiMessagesResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = cli_source_filter.unwrap_or(request.source);
    let mut response = service.messages(
        &[],
        None,
        Some(source),
        MessageQueryOptions::new(
            request.limits.limit,
            request.limits.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    response.peer_results = Some(vec![local_peer_result(
        "read/source",
        Some(response.total_messages),
        None,
    )]);
    Ok(response)
}

fn peer_read_project_local_response(
    service: &QueryService,
    request: PeerProjectRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<ApiMessagesResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = request.source.or(cli_source_filter);
    let project = resolve_peer_project(service, &request.project, source)?;
    let mut response = service.messages(
        &[],
        project.as_deref(),
        source,
        MessageQueryOptions::new(
            request.limits.limit,
            request.limits.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    response.peer_results = Some(vec![local_peer_result(
        "read/project",
        Some(response.total_messages),
        None,
    )]);
    Ok(response)
}

fn peer_context_project_local_response(
    service: &QueryService,
    request: PeerProjectRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<ContextResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = request.source.or(cli_source_filter);
    let project = resolve_peer_project(service, &request.project, source)?;
    let sessions = service.sessions(
        project.as_deref(),
        source,
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    )?;
    let messages = service.messages(
        &[],
        project.as_deref(),
        source,
        MessageQueryOptions::new(
            request.limits.limit,
            request.limits.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    Ok(ContextResponse {
        command: "context/project".to_string(),
        scope: "project".to_string(),
        source: source.map(source_name).map(str::to_string),
        project,
        total_sessions: sessions.total_sessions,
        total_messages: messages.total_messages,
        sessions: sessions.sessions,
        messages: messages.messages,
        peer_results: Some(vec![local_peer_result(
            "context/project",
            Some(messages.total_messages),
            Some(sessions.total_sessions),
        )]),
    })
}

fn peer_context_source_local_response(
    service: &QueryService,
    request: PeerReadSourceRequest,
    cli_source_filter: Option<SourceFilter>,
) -> Result<ContextResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = cli_source_filter.unwrap_or(request.source);
    let sessions = service.sessions(
        None,
        Some(source),
        None,
        0,
        SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
    )?;
    let messages = service.messages(
        &[],
        None,
        Some(source),
        MessageQueryOptions::new(
            request.limits.limit,
            request.limits.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    Ok(ContextResponse {
        command: "context/source".to_string(),
        scope: "source".to_string(),
        source: Some(source_name(source).to_string()),
        project: None,
        total_sessions: sessions.total_sessions,
        total_messages: messages.total_messages,
        sessions: sessions.sessions,
        messages: messages.messages,
        peer_results: Some(vec![local_peer_result(
            "context/source",
            Some(messages.total_messages),
            Some(sessions.total_sessions),
        )]),
    })
}

fn peer_recall_local_response(
    service: &QueryService,
    request: PeerProjectRequest,
    cli_source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<ApiMessagesResponse> {
    validate_peer_protocol(request.protocol_version)?;
    let source = request.source.or(cli_source_filter);
    let recall = request
        .recall
        .ok_or_else(|| anyhow::anyhow!("peer recall request missing recall selector"))?;
    let project = if request.all {
        None
    } else {
        resolve_peer_project(service, &request.project, source)?
    };
    let outcome = service.messages_by_session_age(
        project.as_deref(),
        request.all,
        source,
        &SessionAxis::Back(recall.n),
        recall.include_newest,
        MessageQueryOptions::new(
            request.limits.limit,
            request.limits.offset,
            SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
        ),
    )?;
    let mut response = match outcome {
        Ok(response) => response,
        Err(error) => {
            return Err(anyhow::Error::new(session_selection_cli_failure(
                error, pretty,
            )?));
        }
    };
    response.peer_results = Some(vec![local_peer_result(
        "recall",
        Some(response.total_messages),
        None,
    )]);
    Ok(response)
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

fn local_peer_result(
    command: &str,
    total_messages: Option<i64>,
    total_sessions: Option<i64>,
) -> ApiPeerResult {
    ApiPeerResult {
        host: "local".to_string(),
        transport: "local".to_string(),
        command: command.to_string(),
        status: "ok".to_string(),
        remote_mmr_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        total_messages,
        total_sessions,
    }
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
    dream_runner: StatusDreamRunnerDiagnostic,
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

#[derive(Debug, Serialize)]
struct StatusDreamRunnerDiagnostic {
    runner: String,
    status: String,
    command_configured: bool,
    command_env: String,
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
            source_filter_name(source_filter),
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
        Some(source) => vec![format!(
            "mmr --source {} ingest events",
            source_filter_name(Some(source)).expect("source name")
        )],
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
    let events = store.events_for_project(&project.id, source_filter_name(source_filter), None)?;
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
    let dream_runner = status_dream_runner_diagnostic();
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
    if let Some(action) = &dream_runner.action {
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
        dream_runner,
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
    let source_name = source_filter_name(Some(source)).unwrap_or("unknown");
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

fn status_dream_runner_diagnostic() -> StatusDreamRunnerDiagnostic {
    StatusDreamRunnerDiagnostic {
        runner: "prompt_runbook".to_string(),
        status: "not_required".to_string(),
        command_configured: false,
        command_env: String::new(),
        action: None,
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
        let source_name = source_filter_name(Some(source)).unwrap_or("unknown");
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
    let events = store.events_for_project(&project.id, source_filter_name(source_filter), None)?;
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
struct DreamResponse {
    command: String,
    mode: String,
    scope: String,
    project_id: Option<String>,
    source: Option<String>,
    per_project_limit: Option<usize>,
    since: Option<String>,
    evidence: DreamEvidenceResponse,
    system_prompt: String,
    runbook: Vec<DreamRunbookStep>,
    output_contract: DreamOutputContract,
    guardrails: Vec<String>,
    suggested_commands: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DreamEvidenceResponse {
    access: String,
    included_events: usize,
    omitted_events: usize,
    evidence_hash: String,
    pii_coverage: PiiCoverageStatus,
    events: Vec<DreamEvidence>,
    omitted: Vec<OmittedDreamEvidence>,
}

#[derive(Debug, Serialize)]
struct DreamRunbookStep {
    step: String,
    objective: String,
    instructions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DreamOutputContract {
    format: String,
    required_sections: Vec<String>,
    memory_candidate_fields: Vec<String>,
    refusal_conditions: Vec<String>,
}

fn assimilate_response(
    args: &AssimilateArgs,
    cli_source: Option<SourceFilter>,
) -> Result<DreamResponse> {
    match &args.command {
        AssimilateCommand::Project(args) => assimilate_project_response(args),
        AssimilateCommand::Source(args) => {
            let source = require_explicit_source(cli_source, "assimilate source")?;
            assimilate_source_response(args, source)
        }
    }
}

fn assimilate_project_response(args: &AssimilateProjectArgs) -> Result<DreamResponse> {
    let store = Store::open_default()?;
    let project = linked_project(&store, args.project.as_deref())?;
    let evidence_mode: DreamEvidenceMode = args.evidence_mode.into();
    if evidence_mode == DreamEvidenceMode::LocalRaw && !args.allow_raw_evidence {
        bail!("raw assimilation evidence requires explicit local-only opt-in");
    }
    let bundle = build_evidence_bundle(&store, &project, evidence_mode.clone())?;
    if bundle.events.is_empty() {
        bail!("assimilate project requires at least one shared-safe evidence event");
    }

    Ok(DreamResponse {
        command: "assimilate/project".to_string(),
        mode: "prompt_runbook".to_string(),
        scope: "project".to_string(),
        project_id: Some(project.id.clone()),
        source: None,
        per_project_limit: None,
        since: None,
        evidence: dream_evidence_response(bundle, evidence_mode),
        system_prompt: dream_system_prompt(),
        runbook: dream_runbook(),
        output_contract: dream_output_contract(),
        guardrails: dream_guardrails(),
        suggested_commands: assimilate_project_suggested_commands(&project),
    })
}

fn assimilate_source_response(
    args: &AssimilateSourceArgs,
    source: SourceFilter,
) -> Result<DreamResponse> {
    let store = Store::open_default()?;
    let evidence_mode: DreamEvidenceMode = args.evidence_mode.into();
    if evidence_mode == DreamEvidenceMode::LocalRaw && !args.allow_raw_evidence {
        bail!("raw assimilation evidence requires explicit local-only opt-in");
    }
    let bundle = build_source_evidence_bundle(
        &store,
        source_name(source),
        evidence_mode.clone(),
        args.since.as_deref(),
        Some(args.per_project_limit),
    )?;
    if bundle.events.is_empty() {
        bail!("assimilate source requires at least one shared-safe evidence event");
    }

    Ok(DreamResponse {
        command: "assimilate/source".to_string(),
        mode: "prompt_runbook".to_string(),
        scope: "source".to_string(),
        project_id: None,
        source: Some(source_name(source).to_string()),
        per_project_limit: Some(args.per_project_limit),
        since: args.since.clone(),
        evidence: dream_evidence_response(bundle, evidence_mode),
        system_prompt: source_assimilation_system_prompt(source),
        runbook: source_assimilation_runbook(),
        output_contract: dream_output_contract(),
        guardrails: source_assimilation_guardrails(),
        suggested_commands: source_assimilation_suggested_commands(source),
    })
}

fn dream_evidence_response(
    bundle: crate::dream::DreamEvidenceBundle,
    evidence_mode: DreamEvidenceMode,
) -> DreamEvidenceResponse {
    DreamEvidenceResponse {
        access: match evidence_mode {
            DreamEvidenceMode::SharedSafe => "shared_safe",
            DreamEvidenceMode::LocalRaw => "local_raw",
        }
        .to_string(),
        included_events: bundle.events.len(),
        omitted_events: bundle.omitted_events.len(),
        evidence_hash: bundle.evidence_hash,
        pii_coverage: bundle.pii_coverage,
        events: bundle.events,
        omitted: bundle.omitted_events,
    }
}

fn dream_system_prompt() -> String {
    [
        "You are a Memory Assimilation Agent operating inside an mmr-linked project.",
        "Your job is to turn the supplied evidence bundle into durable, evidence-cited knowledge for future agents.",
        "Perform memory deduplication, knowledge assimilation, and generalisation yourself; do not assume `mmr assimilate project` already ran an AI provider or wrote memory.",
        "Prefer stable operating preferences, repeatable workflow lessons, project facts, and unresolved follow-ups over transcript summary.",
        "Every proposed memory must cite one or more supplied `mmr://event/...` refs and must identify counterevidence when present.",
        "Reject or quarantine claims that are personal, secret-bearing, identity-affecting, unsupported, too narrow to reuse, or contradicted by newer evidence.",
        "When evidence is insufficient, return the smallest missing fact or command needed to continue instead of inventing memory.",
    ]
    .join("\n")
}

fn dream_runbook() -> Vec<DreamRunbookStep> {
    vec![
        DreamRunbookStep {
            step: "deduplicate".to_string(),
            objective: "Collapse repeated or overlapping observations into the smallest durable set."
                .to_string(),
            instructions: vec![
                "Group evidence by recurring decision, preference, workflow, project fact, blocker, or correction.".to_string(),
                "Prefer one generalized memory over several near-duplicates when the same lesson recurs.".to_string(),
                "Keep distinct memories separate when scope, owner, tool, project, or acceptance criteria materially differ.".to_string(),
            ],
        },
        DreamRunbookStep {
            step: "assimilate".to_string(),
            objective: "Convert evidence into reusable knowledge with explicit provenance.".to_string(),
            instructions: vec![
                "Write each candidate as a concise claim that a future agent can act on.".to_string(),
                "Attach all supporting evidence refs and any counterevidence refs.".to_string(),
                "Classify each candidate as active, pending, rejected, superseded, or duplicate.".to_string(),
            ],
        },
        DreamRunbookStep {
            step: "generalise".to_string(),
            objective: "Lift narrow session details into stable rules without losing important boundaries."
                .to_string(),
            instructions: vec![
                "Generalise only when multiple evidence points or strong single evidence justify a reusable rule.".to_string(),
                "Preserve scope limits such as project path, provider, command, file, date, or environment when they matter.".to_string(),
                "Prefer operational wording: what future agents should inspect, run, avoid, or verify.".to_string(),
            ],
        },
        DreamRunbookStep {
            step: "apply-or-report".to_string(),
            objective: "Return actionable output for the caller to apply through the appropriate durable surface."
                .to_string(),
            instructions: vec![
                "If asked to update memory, produce a concrete patch or command sequence rather than vague advice.".to_string(),
                "If no durable memory should be added, explain why and list the evidence refs reviewed.".to_string(),
                "Do not write secrets, raw personal data, or unsupported conclusions into long-term memory.".to_string(),
            ],
        },
    ]
}

fn dream_output_contract() -> DreamOutputContract {
    DreamOutputContract {
        format: "markdown_or_json".to_string(),
        required_sections: vec![
            "evidence_reviewed".to_string(),
            "deduplication_groups".to_string(),
            "memory_candidates".to_string(),
            "counterevidence_or_rejections".to_string(),
            "application_plan".to_string(),
        ],
        memory_candidate_fields: vec![
            "kind".to_string(),
            "claim".to_string(),
            "scope".to_string(),
            "status".to_string(),
            "confidence".to_string(),
            "evidence_refs".to_string(),
            "counterevidence_refs".to_string(),
            "target_surface".to_string(),
        ],
        refusal_conditions: vec![
            "no supplied evidence supports the claim".to_string(),
            "claim contains secrets, credentials, or raw private identifiers".to_string(),
            "claim is better represented as a transient task than durable memory".to_string(),
            "newer evidence contradicts the proposed memory".to_string(),
        ],
    }
}

fn dream_guardrails() -> Vec<String> {
    vec![
        "Do not treat omitted evidence as reviewed.".to_string(),
        "Do not infer private facts from redacted placeholders.".to_string(),
        "Do not create duplicate memory when an existing memory should be superseded or left unchanged.".to_string(),
        "Do not mark a candidate active without at least one supplied evidence ref.".to_string(),
        "Keep project-scoped memories scoped to the project unless the evidence supports a broader account-level preference.".to_string(),
    ]
}

fn assimilate_project_suggested_commands(project: &ProjectRecord) -> Vec<String> {
    let project = shell_quote_path(Path::new(&project.canonical_path));
    vec![
        format!("mmr find --project {project} <query> --pretty"),
        format!("mmr summarize project --project {project} --pretty"),
        format!("mmr sync --project {project} --dry-run --pretty"),
    ]
}

fn source_assimilation_system_prompt(source: SourceFilter) -> String {
    let source = source_name(source);
    [
        format!("You are a Memory Assimilation Agent improving the {source} coding-agent harness across projects."),
        "Your job is to turn the supplied source-wide evidence bundle into durable, evidence-cited harness knowledge.".to_string(),
        "Perform memory deduplication, knowledge assimilation, and generalisation yourself; do not assume `mmr assimilate source` already ran an AI provider or wrote memory.".to_string(),
        "Prefer repeatable harness behavior, steering failures, tool-use lessons, recovery patterns, and source-specific operating constraints over project facts.".to_string(),
        "Generalise across projects only when evidence supports the pattern; keep project-specific facts out of harness memory unless they are necessary counterevidence.".to_string(),
        "Every proposed memory must cite one or more supplied `mmr://event/...` refs and must identify counterevidence when present.".to_string(),
        "When evidence is insufficient, return the smallest missing fact or command needed to continue instead of inventing memory.".to_string(),
    ]
    .join("\n")
}

fn source_assimilation_runbook() -> Vec<DreamRunbookStep> {
    let mut runbook = dream_runbook();
    if let Some(step) = runbook.iter_mut().find(|step| step.step == "generalise") {
        step.instructions.push(
            "Look for cross-project harness patterns before proposing account-wide harness memory."
                .to_string(),
        );
        step.instructions
            .push("Do not turn one project's implementation fact into a harness rule.".to_string());
    }
    runbook
}

fn source_assimilation_guardrails() -> Vec<String> {
    let mut guardrails = dream_guardrails();
    guardrails.push(
        "Keep source-scoped memories about the harness; quarantine project-specific claims."
            .to_string(),
    );
    guardrails
}

fn source_assimilation_suggested_commands(source: SourceFilter) -> Vec<String> {
    let source = source_name(source);
    vec![
        format!("mmr find --source {source} <query> --pretty"),
        format!("mmr context source --source {source} --pretty"),
        format!("mmr read source --source {source} --limit 200 --pretty"),
    ]
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    query: String,
    total_results: usize,
    results: Vec<SearchResult>,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    project_id: String,
    source: String,
    session_id: String,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
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

fn find_output(
    args: &SearchTextArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let response = search_response(args, source_filter)?;
    if args.format == FindFormatArg::Json {
        return serialize(&response, pretty);
    }

    let mut output = String::new();
    for result in response.results {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            result.citation, result.line_number, result.source, result.snippet
        ));
    }
    Ok(output)
}

fn search_response(
    args: &SearchTextArgs,
    source_filter: Option<SourceFilter>,
) -> Result<SearchResponse> {
    let mut store = Store::open_default()?;
    let project = linked_project(&store, args.project.as_deref())?;
    let results = search_project(
        &mut store,
        &project,
        source_filter,
        args.session.as_deref(),
        args.role.as_deref(),
        args.event_type.as_deref(),
        &args.query,
        args.ignore_case,
        args.context,
    )?;

    Ok(SearchResponse {
        query: args.query.clone(),
        total_results: results.len(),
        results,
    })
}

#[allow(clippy::too_many_arguments)]
fn search_project(
    store: &mut Store,
    project: &ProjectRecord,
    source_filter: Option<SourceFilter>,
    session: Option<&str>,
    role: Option<&str>,
    event_type: Option<&str>,
    query: &str,
    ignore_case: bool,
    context: usize,
) -> Result<Vec<SearchResult>> {
    if query.is_empty() {
        bail!("search query is empty");
    }

    let events =
        store.events_for_project(&project.id, source_filter_name(source_filter), session)?;
    let mut results = Vec::new();
    for event in events {
        if role.is_some_and(|role| role != event.role) {
            continue;
        }
        if event_type.is_some_and(|event_type| event_type != event.event_type) {
            continue;
        }

        let search_document = store.upsert_search_document(&event)?;
        for line_match in
            find_line_matches(&search_document.document_text, query, ignore_case, context)
        {
            results.push(SearchResult {
                project_id: event.project_id.clone(),
                source: event.source.clone(),
                session_id: event.session_id.clone(),
                event_id: event.id.clone(),
                event_type: event.event_type.clone(),
                role: event.role.clone(),
                timestamp: event.timestamp.clone(),
                citation: search_document.citation.clone(),
                line_number: line_match.line_number,
                snippet: line_match.snippet,
                before: line_match.before,
                after: line_match.after,
            });
        }
    }
    if source_filter.is_none() && session.is_none() {
        for memory in store.learned_memory_for_project(&project.id)? {
            if memory.status != "active" {
                continue;
            }
            if role.is_some_and(|role| role != "memory") {
                continue;
            }
            if event_type.is_some_and(|event_type| event_type != "learned_memory") {
                continue;
            }
            for line_match in find_line_matches(&memory.claim, query, ignore_case, context) {
                results.push(SearchResult {
                    project_id: memory.project_id.clone(),
                    source: "learned_memory".to_string(),
                    session_id: memory
                        .dream_run_id
                        .clone()
                        .unwrap_or_else(|| "learned_memory".to_string()),
                    event_id: memory.id.clone(),
                    event_type: "learned_memory".to_string(),
                    role: "memory".to_string(),
                    timestamp: memory.created_at.clone(),
                    citation: format!("mmr://learned-memory/{}", memory.id),
                    line_number: line_match.line_number,
                    snippet: line_match.snippet,
                    before: line_match.before,
                    after: line_match.after,
                });
            }
        }
    }

    results.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
    Ok(results)
}

#[derive(Debug, Serialize)]
struct RetrieveResponse {
    query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    debug: Option<RetrieveDebugOutput>,
    total_matches: usize,
    total_selected_sessions: usize,
    selected_sessions: Vec<RetrieveSelectedSession>,
    unreadable_matches: Vec<RetrieveUnreadableMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_command: Option<Option<String>>,
    suggested_next_action: Option<String>,
}

#[derive(Debug, Serialize)]
struct RetrieveDebugOutput {
    limits: RetrieveLimits,
    scope: RetrieveScopeOutput,
    total_ranked_sessions: usize,
}

#[derive(Debug, Clone, Serialize)]
struct RetrieveLimits {
    max_sessions: usize,
    before_messages: usize,
    after_messages: usize,
    max_messages_per_session: usize,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Serialize)]
struct RetrieveScopeOutput {
    all_projects: bool,
    all_sources: bool,
    source_filter: Option<String>,
    total_projects_searched: usize,
    projects: Vec<String>,
}

impl RetrieveLimits {
    fn from_args(args: &RetrieveArgs) -> Self {
        let limit = args.limit.unwrap_or_else(|| {
            args.max_sessions
                .saturating_mul(args.max_messages_per_session)
        });
        Self {
            max_sessions: args.max_sessions,
            before_messages: args.before_messages,
            after_messages: args.after_messages,
            max_messages_per_session: args.max_messages_per_session,
            limit,
            offset: args.offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetrieveSessionIdentity {
    source: String,
    project_name: String,
    source_session_id: String,
}

#[derive(Debug, Serialize)]
struct RetrieveSelectedSession {
    rank: usize,
    source: String,
    project_name: String,
    source_session_id: String,
    rank_reason: RetrieveRankReason,
    match_count: usize,
    first_match_citation: String,
    matches: Vec<RetrieveMatchOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_window: Option<RetrieveMessageWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    messages: Option<Vec<ApiMessage>>,
}

#[derive(Debug, Serialize)]
struct RetrieveRankReason {
    match_count: usize,
    latest_match_timestamp: String,
    tie_break: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RetrieveMessageWindow {
    before_messages: usize,
    after_messages: usize,
    max_messages_per_session: usize,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RetrieveMatchOutput {
    source: String,
    project_name: String,
    source_session_id: String,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RetrieveUnreadableMatch {
    source: String,
    project_name: String,
    source_session_id: Option<String>,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
    reason: String,
}

#[derive(Debug, Clone)]
struct RetrieveMatchRecord {
    identity: RetrieveSessionIdentity,
    event_id: String,
    event_type: String,
    role: String,
    timestamp: String,
    citation: String,
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
}

impl RetrieveMatchRecord {
    fn output(&self) -> RetrieveMatchOutput {
        RetrieveMatchOutput {
            source: self.identity.source.clone(),
            project_name: self.identity.project_name.clone(),
            source_session_id: self.identity.source_session_id.clone(),
            event_id: public_retrieve_event_id(&self.event_id),
            event_type: self.event_type.clone(),
            role: self.role.clone(),
            timestamp: self.timestamp.clone(),
            citation: self.citation.clone(),
            line_number: self.line_number,
            snippet: self.snippet.clone(),
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }

    fn unreadable(&self, reason: &str) -> RetrieveUnreadableMatch {
        RetrieveUnreadableMatch {
            source: self.identity.source.clone(),
            project_name: self.identity.project_name.clone(),
            source_session_id: Some(self.identity.source_session_id.clone()),
            event_id: public_retrieve_event_id(&self.event_id),
            event_type: self.event_type.clone(),
            role: self.role.clone(),
            timestamp: self.timestamp.clone(),
            citation: self.citation.clone(),
            line_number: self.line_number,
            snippet: self.snippet.clone(),
            before: self.before.clone(),
            after: self.after.clone(),
            reason: reason.to_string(),
        }
    }
}

#[derive(Debug)]
struct RetrieveSearchMatches {
    event_matches: Vec<RetrieveMatchRecord>,
    learned_matches: Vec<RetrieveUnreadableMatch>,
}

#[derive(Debug)]
struct RetrieveWindow {
    messages: Vec<ApiMessage>,
    truncated: bool,
}

#[derive(Debug)]
struct RetrieveSessionCandidate {
    identity: RetrieveSessionIdentity,
    selected: RetrieveSelectedSession,
    message_window: RetrieveMessageWindow,
    message_history: Vec<ApiMessage>,
    match_count: usize,
    latest_match_timestamp: String,
}

fn retrieve_output(
    args: &RetrieveArgs,
    service: &QueryService,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    let limits = RetrieveLimits::from_args(args);
    if args.query.is_empty() {
        return Err(anyhow::Error::new(retrieve_cli_failure(
            "invalid_query",
            "retrieve query is empty",
            pretty,
        )?));
    }

    let pinned_sessions = parse_retrieve_pinned_sessions(args, pretty)?;
    let mut store = Store::open_default()?;
    let source_filter = retrieve_effective_source_filter(args, cli_source, source_filter, pretty)?;
    let store_projects = retrieve_scope_projects(&store, args, pretty)?;
    let matches = if args.all_projects {
        retrieve_all_projects_matches(service, &mut store, &store_projects, source_filter, args)?
    } else {
        retrieve_search_matches(&mut store, &store_projects, source_filter, args)?
    };
    let total_matches = matches.event_matches.len() + matches.learned_matches.len();

    let mut grouped = group_retrieve_matches(matches.event_matches);
    if !pinned_sessions.is_empty() {
        let pinned_set = pinned_sessions.iter().cloned().collect::<BTreeSet<_>>();
        for identity in &pinned_sessions {
            if !grouped.contains_key(identity) {
                return Err(anyhow::Error::new(retrieve_cli_failure(
                    "pinned_session_not_found",
                    format!(
                        "pinned session {}:{}:{} is not present in the current retrieve scope",
                        identity.source, identity.project_name, identity.source_session_id
                    ),
                    pretty,
                )?));
            }
        }
        grouped.retain(|identity, _| pinned_set.contains(identity));
    }

    let mut unreadable_matches = matches.learned_matches;
    let mut candidates = Vec::new();
    for (identity, mut session_matches) in grouped {
        sort_retrieve_matches(&mut session_matches);
        let Some(provider_source) = source_filter_from_name(identity.source.as_str()) else {
            unreadable_matches.extend(
                session_matches
                    .iter()
                    .map(|item| item.unreadable("unsupported_source")),
            );
            continue;
        };

        let provider_messages =
            retrieve_provider_messages(service, &identity, provider_source).unwrap_or_default();
        if provider_messages.is_empty() {
            unreadable_matches.extend(
                session_matches
                    .iter()
                    .map(|item| item.unreadable("provider_transcript_not_found")),
            );
            continue;
        }

        candidates.push(build_retrieve_session_candidate(
            args,
            &limits,
            identity,
            session_matches,
            provider_messages,
        ));
    }

    candidates.sort_by(|left, right| {
        right
            .match_count
            .cmp(&left.match_count)
            .then_with(|| {
                right
                    .latest_match_timestamp
                    .cmp(&left.latest_match_timestamp)
            })
            .then_with(|| left.identity.source.cmp(&right.identity.source))
            .then_with(|| left.identity.project_name.cmp(&right.identity.project_name))
            .then_with(|| {
                left.identity
                    .source_session_id
                    .cmp(&right.identity.source_session_id)
            })
    });

    let total_ranked_sessions = candidates.len();
    candidates.truncate(limits.max_sessions);
    assign_retrieve_ranks(&mut candidates);
    let (next_page, next_offset) = if args.full_message_history {
        apply_retrieve_flattened_pagination(&mut candidates, &limits)
    } else {
        (false, limits.offset)
    };
    let selected_identities = candidates
        .iter()
        .map(|candidate| candidate.identity.clone())
        .collect::<Vec<_>>();
    let next_command = if args.full_message_history && next_page {
        Some(build_retrieve_next_command(
            args,
            &limits,
            source_filter,
            &selected_identities,
            next_offset,
        )?)
    } else {
        None
    };
    let debug = if args.debug {
        let scope_projects =
            retrieve_scope_project_names(service, &store_projects, source_filter, args);
        Some(RetrieveDebugOutput {
            limits: limits.clone(),
            scope: RetrieveScopeOutput {
                all_projects: args.all_projects,
                all_sources: source_filter.is_none(),
                source_filter: source_filter.map(source_name).map(str::to_string),
                total_projects_searched: scope_projects.len(),
                projects: scope_projects,
            },
            total_ranked_sessions,
        })
    } else {
        None
    };
    let selected_sessions = candidates
        .into_iter()
        .map(|mut candidate| {
            if args.full_message_history {
                candidate.selected.message_window = Some(candidate.message_window);
                candidate.selected.messages = Some(candidate.message_history);
            }
            candidate.selected
        })
        .collect::<Vec<_>>();

    let response = RetrieveResponse {
        query: args.query.clone(),
        debug,
        total_matches,
        total_selected_sessions: selected_sessions.len(),
        selected_sessions,
        unreadable_matches,
        next_page: args.full_message_history.then_some(next_page),
        next_offset: args.full_message_history.then_some(next_offset),
        next_command: args.full_message_history.then_some(next_command),
        suggested_next_action: suggested_retrieve_next_action(total_matches),
    };
    serialize(&response, pretty)
}

fn retrieve_effective_source_filter(
    args: &RetrieveArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<Option<SourceFilter>> {
    if args.all_sources && cli_source.is_some() {
        return Err(anyhow::Error::new(retrieve_cli_failure(
            "invalid_scope_flags",
            "--all-sources cannot be combined with --source",
            pretty,
        )?));
    }
    if args.all_sources {
        Ok(None)
    } else {
        Ok(source_filter)
    }
}

fn retrieve_scope_projects(
    store: &Store,
    args: &RetrieveArgs,
    pretty: bool,
) -> Result<Vec<ProjectRecord>> {
    if args.all_projects && args.project.is_some() {
        return Err(anyhow::Error::new(retrieve_cli_failure(
            "invalid_scope_flags",
            "--all-projects cannot be combined with --project",
            pretty,
        )?));
    }
    if args.all_projects {
        return store.projects();
    }
    Ok(vec![linked_project(store, args.project.as_deref())?])
}

fn retrieve_scope_project_names(
    service: &QueryService,
    store_projects: &[ProjectRecord],
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Vec<String> {
    if !args.all_projects {
        return store_projects
            .iter()
            .map(|project| project.canonical_path.clone())
            .collect();
    }

    let mut projects = service
        .projects(
            source_filter,
            None,
            0,
            SortOptions::new(SortBy::Timestamp, SortOrder::Desc),
        )
        .projects
        .into_iter()
        .map(|project| project.name)
        .collect::<Vec<_>>();
    projects.extend(
        store_projects
            .iter()
            .map(|project| project.canonical_path.clone()),
    );
    projects.sort();
    projects.dedup();
    projects
}

fn parse_retrieve_pinned_sessions(
    args: &RetrieveArgs,
    pretty: bool,
) -> Result<Vec<RetrieveSessionIdentity>> {
    let mut identities = Vec::new();
    for raw in &args.pinned_sessions {
        let identity = serde_json::from_str::<RetrieveSessionIdentity>(raw).map_err(|error| {
            anyhow::Error::new(
                retrieve_cli_failure(
                    "invalid_pinned_session",
                    format!("invalid pinned session JSON: {error}"),
                    pretty,
                )
                .expect("serialize retrieve pinned error"),
            )
        })?;
        if identity.source.trim().is_empty()
            || identity.project_name.trim().is_empty()
            || identity.source_session_id.trim().is_empty()
            || source_filter_from_name(identity.source.as_str()).is_none()
        {
            return Err(anyhow::Error::new(retrieve_cli_failure(
                "invalid_pinned_session",
                "pinned session must include source, project_name, and source_session_id",
                pretty,
            )?));
        }
        identities.push(identity);
    }
    Ok(identities)
}

fn retrieve_cli_failure(
    error_kind: &str,
    message: impl Into<String>,
    pretty: bool,
) -> Result<CliFailure> {
    let message = message.into();
    let value = serde_json::json!({
        "error_kind": error_kind,
        "message": message,
    });
    Ok(CliFailure::new(2, serialize(&value, pretty)?, message))
}

fn retrieve_all_projects_matches(
    service: &QueryService,
    store: &mut Store,
    store_projects: &[ProjectRecord],
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Result<RetrieveSearchMatches> {
    let mut event_matches = retrieve_provider_message_matches(service, source_filter, args)?;
    let mut store_matches = retrieve_search_matches(store, store_projects, source_filter, args)?;

    let provider_identities = event_matches
        .iter()
        .map(|item| item.identity.clone())
        .collect::<BTreeSet<_>>();
    for item in store_matches.event_matches.drain(..) {
        if provider_identities.contains(&item.identity) {
            continue;
        }
        store_matches
            .learned_matches
            .push(item.unreadable("provider_transcript_not_found"));
    }

    sort_retrieve_matches(&mut event_matches);
    store_matches.learned_matches.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
    Ok(RetrieveSearchMatches {
        event_matches,
        learned_matches: store_matches.learned_matches,
    })
}

fn retrieve_provider_message_matches(
    service: &QueryService,
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Result<Vec<RetrieveMatchRecord>> {
    if args
        .event_type
        .as_deref()
        .is_some_and(|event_type| event_type != "message")
    {
        return Ok(Vec::new());
    }

    let session_ids = args.session.iter().cloned().collect::<Vec<_>>();
    let response = service.messages(
        &session_ids,
        None,
        source_filter,
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    let matches = response
        .messages
        .par_iter()
        .flat_map(|message| {
            if args
                .role
                .as_deref()
                .is_some_and(|role| role != message.role)
            {
                return Vec::new();
            }
            find_line_matches(
                &message.content,
                &args.query,
                args.ignore_case,
                args.context,
            )
            .into_iter()
            .map(|line_match| retrieve_match_from_provider_message(message, line_match))
            .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    Ok(matches)
}

fn retrieve_search_matches(
    store: &mut Store,
    projects: &[ProjectRecord],
    source_filter: Option<SourceFilter>,
    args: &RetrieveArgs,
) -> Result<RetrieveSearchMatches> {
    let mut event_matches = Vec::new();
    for project in projects {
        let events = store.events_for_project(
            &project.id,
            source_filter_name(source_filter),
            args.session.as_deref(),
        )?;
        for event in events {
            if args.role.as_deref().is_some_and(|role| role != event.role) {
                continue;
            }
            if args
                .event_type
                .as_deref()
                .is_some_and(|event_type| event_type != event.event_type)
            {
                continue;
            }

            let search_document = store.upsert_search_document(&event)?;
            for line_match in find_line_matches(
                &search_document.document_text,
                &args.query,
                args.ignore_case,
                args.context,
            ) {
                event_matches.push(retrieve_match_from_event(
                    project,
                    &event,
                    &search_document.citation,
                    line_match,
                ));
            }
        }
    }

    let mut learned_matches = Vec::new();
    if source_filter.is_none() && args.session.is_none() {
        for project in projects {
            for memory in store.learned_memory_for_project(&project.id)? {
                if memory.status != "active" {
                    continue;
                }
                if args.role.as_deref().is_some_and(|role| role != "memory") {
                    continue;
                }
                if args
                    .event_type
                    .as_deref()
                    .is_some_and(|event_type| event_type != "learned_memory")
                {
                    continue;
                }
                for line_match in
                    find_line_matches(&memory.claim, &args.query, args.ignore_case, args.context)
                {
                    learned_matches.push(RetrieveUnreadableMatch {
                        source: "learned_memory".to_string(),
                        project_name: project.canonical_path.clone(),
                        source_session_id: memory.dream_run_id.clone(),
                        event_id: memory.id.clone(),
                        event_type: "learned_memory".to_string(),
                        role: "memory".to_string(),
                        timestamp: memory.created_at.clone(),
                        citation: format!("mmr://learned-memory/{}", memory.id),
                        line_number: line_match.line_number,
                        snippet: line_match.snippet,
                        before: line_match.before,
                        after: line_match.after,
                        reason: "learned_memory_match".to_string(),
                    });
                }
            }
        }
    }

    sort_retrieve_matches(&mut event_matches);
    learned_matches.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
    Ok(RetrieveSearchMatches {
        event_matches,
        learned_matches,
    })
}

fn retrieve_match_from_event(
    project: &ProjectRecord,
    event: &EventRecord,
    citation: &str,
    line_match: LineMatch,
) -> RetrieveMatchRecord {
    RetrieveMatchRecord {
        identity: RetrieveSessionIdentity {
            source: event.source.clone(),
            project_name: project.canonical_path.clone(),
            source_session_id: event.source_session_id.clone(),
        },
        event_id: event.id.clone(),
        event_type: event.event_type.clone(),
        role: event.role.clone(),
        timestamp: event.timestamp.clone(),
        citation: citation.to_string(),
        line_number: line_match.line_number,
        snippet: line_match.snippet,
        before: line_match.before,
        after: line_match.after,
    }
}

fn retrieve_match_from_provider_message(
    message: &ApiMessage,
    line_match: LineMatch,
) -> RetrieveMatchRecord {
    let event_id = provider_message_event_id(message, line_match.line_number);
    RetrieveMatchRecord {
        identity: RetrieveSessionIdentity {
            source: message.source.clone(),
            project_name: message.project_name.clone(),
            source_session_id: message.session_id.clone(),
        },
        event_id: event_id.clone(),
        event_type: "message".to_string(),
        role: message.role.clone(),
        timestamp: message.timestamp.clone(),
        citation: format!("mmr://message/{event_id}"),
        line_number: line_match.line_number,
        snippet: line_match.snippet,
        before: line_match.before,
        after: line_match.after,
    }
}

fn provider_message_event_id(message: &ApiMessage, line_number: usize) -> String {
    let hash = content_hash(&format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        message.source,
        message.project_name,
        message.session_id,
        message.role,
        message.timestamp,
        line_number,
        message.content
    ));
    format!(
        "message:v1:{}",
        hash.strip_prefix("sha256:").unwrap_or(hash.as_str())
    )
}

fn group_retrieve_matches(
    matches: Vec<RetrieveMatchRecord>,
) -> BTreeMap<RetrieveSessionIdentity, Vec<RetrieveMatchRecord>> {
    let mut grouped: BTreeMap<RetrieveSessionIdentity, Vec<RetrieveMatchRecord>> = BTreeMap::new();
    for item in matches {
        grouped.entry(item.identity.clone()).or_default().push(item);
    }
    grouped
}

fn sort_retrieve_matches(matches: &mut [RetrieveMatchRecord]) {
    matches.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.event_id.cmp(&right.event_id))
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
}

fn retrieve_provider_messages(
    service: &QueryService,
    identity: &RetrieveSessionIdentity,
    source: SourceFilter,
) -> Result<Vec<ApiMessage>> {
    let response = service.messages(
        std::slice::from_ref(&identity.source_session_id),
        Some(identity.project_name.as_str()),
        Some(source),
        MessageQueryOptions::new(None, 0, SortOptions::new(SortBy::Timestamp, SortOrder::Asc)),
    )?;
    Ok(response.messages)
}

fn build_retrieve_session_candidate(
    args: &RetrieveArgs,
    limits: &RetrieveLimits,
    identity: RetrieveSessionIdentity,
    matches: Vec<RetrieveMatchRecord>,
    provider_messages: Vec<ApiMessage>,
) -> RetrieveSessionCandidate {
    let latest_match_timestamp = matches
        .iter()
        .map(|item| item.timestamp.as_str())
        .max()
        .unwrap_or_default()
        .to_string();
    let rank_reason = RetrieveRankReason {
        match_count: matches.len(),
        latest_match_timestamp: latest_match_timestamp.clone(),
        tie_break: vec![
            identity.source.clone(),
            identity.project_name.clone(),
            identity.source_session_id.clone(),
        ],
    };
    let first_match_citation = matches
        .first()
        .map(|item| item.citation.clone())
        .unwrap_or_default();
    let RetrieveWindow {
        messages,
        truncated,
    } = build_retrieve_message_window(args, limits, &matches, provider_messages);
    let selected = RetrieveSelectedSession {
        rank: 0,
        source: identity.source.clone(),
        project_name: identity.project_name.clone(),
        source_session_id: identity.source_session_id.clone(),
        rank_reason,
        match_count: matches.len(),
        first_match_citation,
        matches: matches.iter().map(RetrieveMatchRecord::output).collect(),
        message_window: None,
        messages: None,
    };
    let message_window = RetrieveMessageWindow {
        before_messages: limits.before_messages,
        after_messages: limits.after_messages,
        max_messages_per_session: limits.max_messages_per_session,
        truncated,
    };

    RetrieveSessionCandidate {
        identity,
        selected,
        message_window,
        message_history: messages,
        match_count: matches.len(),
        latest_match_timestamp,
    }
}

fn assign_retrieve_ranks(candidates: &mut [RetrieveSessionCandidate]) {
    for (idx, candidate) in candidates.iter_mut().enumerate() {
        candidate.selected.rank = idx + 1;
        candidate.selected.match_count = candidate.match_count;
    }
}

fn apply_retrieve_flattened_pagination(
    candidates: &mut [RetrieveSessionCandidate],
    limits: &RetrieveLimits,
) -> (bool, usize) {
    let full_messages = candidates
        .iter()
        .map(|candidate| candidate.message_history.clone())
        .collect::<Vec<_>>();
    let flattened = full_messages
        .iter()
        .enumerate()
        .flat_map(|(session_idx, messages)| {
            messages
                .iter()
                .enumerate()
                .map(move |(message_idx, _)| (session_idx, message_idx))
        })
        .collect::<Vec<_>>();
    let total = flattened.len();
    let page_start = limits.offset.min(total);
    let page_end = page_start.saturating_add(limits.limit).min(total);

    for candidate in candidates.iter_mut() {
        candidate.message_history.clear();
    }
    for (session_idx, message_idx) in &flattened[page_start..page_end] {
        if let Some(message) = full_messages
            .get(*session_idx)
            .and_then(|messages| messages.get(*message_idx))
        {
            candidates[*session_idx]
                .message_history
                .push(message.clone());
        }
    }

    let next_page = limits.limit > 0 && page_end < total;
    let next_offset = if next_page { page_end } else { limits.offset };
    (next_page, next_offset)
}

fn build_retrieve_message_window(
    args: &RetrieveArgs,
    limits: &RetrieveLimits,
    matches: &[RetrieveMatchRecord],
    messages: Vec<ApiMessage>,
) -> RetrieveWindow {
    if messages.is_empty() {
        return RetrieveWindow {
            messages,
            truncated: false,
        };
    }

    let mut anchors = BTreeSet::new();
    for item in matches {
        anchors.insert(find_retrieve_anchor_index(
            &messages,
            item,
            &args.query,
            args.ignore_case,
        ));
    }
    if anchors.is_empty() {
        anchors.insert(0);
    }

    let mut window_indices = BTreeSet::new();
    for anchor in &anchors {
        let start = anchor.saturating_sub(limits.before_messages);
        let end = anchor
            .saturating_add(limits.after_messages)
            .saturating_add(1)
            .min(messages.len());
        for idx in start..end {
            window_indices.insert(idx);
        }
    }

    let raw_len = window_indices.len();
    let mut capped_indices = BTreeSet::new();
    for anchor in &anchors {
        if capped_indices.len() >= limits.max_messages_per_session {
            break;
        }
        if window_indices.contains(anchor) {
            capped_indices.insert(*anchor);
        }
    }
    for idx in window_indices {
        if capped_indices.len() >= limits.max_messages_per_session {
            break;
        }
        capped_indices.insert(idx);
    }

    let truncated = raw_len > capped_indices.len();
    let selected = capped_indices
        .into_iter()
        .filter_map(|idx| messages.get(idx).cloned())
        .collect::<Vec<_>>();
    RetrieveWindow {
        messages: selected,
        truncated,
    }
}

fn find_retrieve_anchor_index(
    messages: &[ApiMessage],
    item: &RetrieveMatchRecord,
    query: &str,
    ignore_case: bool,
) -> usize {
    if let Some((idx, _)) = messages.iter().enumerate().find(|(_, message)| {
        message.timestamp == item.timestamp
            && contains_retrieve_text(&message.content, query, ignore_case)
    }) {
        return idx;
    }
    if let Some((idx, _)) = messages
        .iter()
        .enumerate()
        .find(|(_, message)| message.timestamp == item.timestamp)
    {
        return idx;
    }
    if let Some((idx, _)) = messages
        .iter()
        .enumerate()
        .find(|(_, message)| contains_retrieve_text(&message.content, query, ignore_case))
    {
        return idx;
    }
    messages
        .iter()
        .enumerate()
        .find(|(_, message)| message.timestamp >= item.timestamp)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| messages.len().saturating_sub(1))
}

fn contains_retrieve_text(haystack: &str, needle: &str, ignore_case: bool) -> bool {
    if ignore_case {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    } else {
        haystack.contains(needle)
    }
}

fn build_retrieve_next_command(
    args: &RetrieveArgs,
    limits: &RetrieveLimits,
    source_filter: Option<SourceFilter>,
    identities: &[RetrieveSessionIdentity],
    next_offset: usize,
) -> Result<String> {
    let mut parts = vec!["mmr".to_string()];
    if let Some(source) = source_filter {
        parts.push("--source".to_string());
        parts.push(source_name(source).to_string());
    }
    parts.push("retrieve".to_string());
    parts.push(shell_quote(&args.query));
    if args.debug {
        parts.push("--debug".to_string());
    }
    if args.full_message_history {
        parts.push("--full-message-history".to_string());
    }
    if args.all_sources {
        parts.push("--all-sources".to_string());
    }

    if args.all_projects {
        parts.push("--all-projects".to_string());
    } else if let Some(project) = args
        .project
        .as_ref()
        .and_then(|path| path.to_str().map(str::to_string))
        .or_else(|| {
            identities
                .first()
                .map(|identity| identity.project_name.clone())
        })
    {
        parts.push("--project".to_string());
        parts.push(shell_quote(&project));
    }
    if let Some(session) = &args.session {
        parts.push("--session".to_string());
        parts.push(shell_quote(session));
    }
    if let Some(role) = &args.role {
        parts.push("--role".to_string());
        parts.push(shell_quote(role));
    }
    if let Some(event_type) = &args.event_type {
        parts.push("--event-type".to_string());
        parts.push(shell_quote(event_type));
    }
    if args.ignore_case {
        parts.push("--ignore-case".to_string());
    }
    if args.context > 0 {
        parts.push("-C".to_string());
        parts.push(args.context.to_string());
    }
    parts.push("--max-sessions".to_string());
    parts.push(limits.max_sessions.to_string());
    parts.push("--before-messages".to_string());
    parts.push(limits.before_messages.to_string());
    parts.push("--after-messages".to_string());
    parts.push(limits.after_messages.to_string());
    parts.push("--max-messages-per-session".to_string());
    parts.push(limits.max_messages_per_session.to_string());
    parts.push("--limit".to_string());
    parts.push(limits.limit.to_string());
    parts.push("--offset".to_string());
    parts.push(next_offset.to_string());

    for identity in identities {
        parts.push("--pinned-session".to_string());
        parts.push(shell_quote(&serde_json::to_string(identity)?));
    }

    Ok(parts.join(" "))
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

fn source_filter_from_name(source: &str) -> Option<SourceFilter> {
    match source {
        "claude" => Some(SourceFilter::Claude),
        "codex" => Some(SourceFilter::Codex),
        "cursor" => Some(SourceFilter::Cursor),
        "grok" => Some(SourceFilter::Grok),
        "pi" => Some(SourceFilter::Pi),
        _ => None,
    }
}

fn public_retrieve_event_id(event_id: &str) -> String {
    event_id
        .strip_prefix("evt:v1:")
        .map(|suffix| format!("event:v1:{suffix}"))
        .unwrap_or_else(|| event_id.to_string())
}

fn suggested_retrieve_next_action(total_matches: usize) -> Option<String> {
    if total_matches == 0 {
        Some(
            "No normalized matches found. Try --ignore-case, a shorter query, or mmr find for raw match diagnostics."
                .to_string(),
        )
    } else {
        None
    }
}

#[derive(Debug)]
struct LineMatch {
    line_number: usize,
    snippet: String,
    before: Vec<String>,
    after: Vec<String>,
}

fn find_line_matches(
    document_text: &str,
    query: &str,
    ignore_case: bool,
    context: usize,
) -> Vec<LineMatch> {
    let lines = document_text.lines().collect::<Vec<_>>();
    let query_cmp = if ignore_case {
        query.to_lowercase()
    } else {
        query.to_string()
    };
    let mut matches = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let line_cmp = if ignore_case {
            line.to_lowercase()
        } else {
            (*line).to_string()
        };
        if !line_cmp.contains(&query_cmp) {
            continue;
        }
        let before_start = idx.saturating_sub(context);
        let after_end = (idx + 1 + context).min(lines.len());
        matches.push(LineMatch {
            line_number: idx + 1,
            snippet: truncate_snippet(line),
            before: lines[before_start..idx]
                .iter()
                .map(|line| truncate_snippet(line))
                .collect(),
            after: lines[idx + 1..after_end]
                .iter()
                .map(|line| truncate_snippet(line))
                .collect(),
        });
    }

    matches
}

fn truncate_snippet(line: &str) -> String {
    const MAX_SNIPPET_CHARS: usize = 500;
    let mut snippet = line.chars().take(MAX_SNIPPET_CHARS).collect::<String>();
    if line.chars().count() > MAX_SNIPPET_CHARS {
        snippet.push_str("...");
    }
    snippet
}

fn export_tree_project_response(
    project: Option<String>,
    output_dir: Option<PathBuf>,
    source_filter: Option<SourceFilter>,
) -> Result<ExportTreeResponse> {
    let store = Store::open_default()?;
    let project_path = project.as_deref().map(PathBuf::from);
    let project = linked_project(&store, project_path.as_deref())?;
    let events = store.events_for_project(&project.id, source_filter_name(source_filter), None)?;
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
            source_filter_name(source_filter),
        )?;
        return serde_json::to_value(response).context("serialize sync response");
    }

    let events = store.events_for_project(&project.id, source_filter_name(source_filter), None)?;

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
    let events = store.events_for_project(&project.id, source_filter_name(source_filter), None)?;
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

fn source_filter_name(source_filter: Option<SourceFilter>) -> Option<&'static str> {
    match source_filter {
        Some(SourceFilter::Claude) => Some("claude"),
        Some(SourceFilter::Codex) => Some("codex"),
        Some(SourceFilter::Cursor) => Some("cursor"),
        Some(SourceFilter::Grok) => Some("grok"),
        Some(SourceFilter::Pi) => Some("pi"),
        None => None,
    }
}

fn source_name(source: SourceFilter) -> &'static str {
    source_filter_name(Some(source)).expect("source name")
}

fn require_explicit_source(source: Option<SourceFilter>, command: &str) -> Result<SourceFilter> {
    source.ok_or_else(|| {
        anyhow::anyhow!("`mmr {command}` requires --source <claude|codex|cursor|grok|pi>")
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
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "claude" => Some(SourceFilter::Claude),
        "codex" => Some(SourceFilter::Codex),
        "cursor" => Some(SourceFilter::Cursor),
        "grok" => Some(SourceFilter::Grok),
        "pi" => Some(SourceFilter::Pi),
        _ => None,
    }
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

#[cfg(test)]
fn validate_message_index_range(
    from: Option<usize>,
    to: Option<usize>,
) -> Result<Option<MessageIndexRange>> {
    if let (Some(from), Some(to)) = (from, to)
        && from > to
    {
        bail!("--from-message-index must be less than or equal to --to-message-index");
    }
    Ok(MessageIndexRange::new(from, to))
}

/// Parse a `--session-range FROM..TO` argument into an inclusive span of recency ages.
///
/// The argument is written older-bound `..` newer-bound (e.g. `2..1` selects ages 1 and 2),
/// so `FROM >= TO >= 1`. Age 0 (the newest, assumed-live session) is never range-addressable.
/// Returns the ages as `newest_bound..=oldest_bound` (`1..=2` for `2..1`).
#[cfg(test)]
fn parse_session_range(input: &str) -> Result<std::ops::RangeInclusive<u32>> {
    let trimmed = input.trim();
    let (oldest_str, newest_str) = trimmed.split_once("..").with_context(|| {
        format!("--session-range expects FROM..TO (older..newer), e.g. 2..1; got {input:?}")
    })?;
    let oldest: u32 = oldest_str.trim().parse().map_err(|_| {
        anyhow::anyhow!("--session-range FROM must be a non-negative integer; got {oldest_str:?}")
    })?;
    let newest: u32 = newest_str.trim().parse().map_err(|_| {
        anyhow::anyhow!("--session-range TO must be a non-negative integer; got {newest_str:?}")
    })?;
    if newest < 1 {
        bail!(
            "--session-range bounds must be >= 1; the newest session is age 0 and is not \
             range-addressable (use --session-back 0 --include-newest); got {input:?}"
        );
    }
    if oldest < newest {
        bail!(
            "--session-range must be written older..newer with FROM >= TO (e.g. 2..1); got {input:?}"
        );
    }
    Ok(newest..=oldest)
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

#[allow(clippy::too_many_arguments)]
fn build_next_messages_command(
    source: Option<SourceFilter>,
    pretty: bool,
    session: &[String],
    project: Option<&str>,
    all: bool,
    message_index_range: Option<MessageIndexRange>,
    limit: usize,
    next_offset: usize,
    sort_by: SortBy,
    order: SortOrder,
) -> String {
    let mut parts = vec!["mmr".to_string()];

    if pretty {
        parts.push("--pretty".to_string());
    }
    if let Some(s) = source {
        let name = match s {
            SourceFilter::Claude => "claude",
            SourceFilter::Codex => "codex",
            SourceFilter::Cursor => "cursor",
            SourceFilter::Grok => "grok",
            SourceFilter::Pi => "pi",
        };
        parts.push(format!("--source {name}"));
    }

    if session.len() == 1 {
        parts.push("read session".to_string());
        parts.push(session[0].clone());
    } else {
        parts.push("read project".to_string());
    }
    if let Some(proj) = project {
        parts.push(format!("--project {proj}"));
    }
    if all {
        parts.push("--all".to_string());
    }
    if let Some(range) = message_index_range {
        if let Some(from) = range.from {
            parts.push(format!("--from-message-index {from}"));
        }
        if let Some(to) = range.to {
            parts.push(format!("--to-message-index {to}"));
        }
    }

    parts.push(format!("--limit {limit}"));
    parts.push(format!("--offset {next_offset}"));

    if sort_by != SortBy::Timestamp {
        let name = match sort_by {
            SortBy::Timestamp => "timestamp",
            SortBy::MessageCount => "message-count",
        };
        parts.push(format!("--sort-by {name}"));
    }
    if order != SortOrder::Asc {
        parts.push("--order desc".to_string());
    }

    parts.join(" ")
}

#[allow(clippy::too_many_arguments)]
fn build_next_recall_command_with_remotes(
    source: Option<SourceFilter>,
    pretty: bool,
    project: Option<&str>,
    all: bool,
    remotes: &[String],
    n: u32,
    include_newest: bool,
    limit: usize,
    next_offset: usize,
) -> String {
    let mut parts = vec!["mmr".to_string()];
    if pretty {
        parts.push("--pretty".to_string());
    }
    if let Some(source) = source {
        parts.push(format!("--source {}", source_name(source)));
    }
    parts.push("recall".to_string());
    if let Some(project) = project {
        parts.push(format!("--project {project}"));
    }
    if all {
        parts.push("--all".to_string());
    }
    for remote in remotes {
        parts.push(format!("--remote {remote}"));
    }
    if include_newest {
        parts.push("--include-newest".to_string());
    }
    parts.push(n.to_string());
    parts.push(format!("--limit {limit}"));
    parts.push(format!("--offset {next_offset}"));
    parts.join(" ")
}

/// Resolve and serialize a reverse session-axis query (`--session-back`/`--session-range`/`prev`).
///
/// Recency-derived ages are unstable across time, so a paged result pins `next_command` to the
/// concrete resolved session id(s) rather than echoing the age-based selector; a session landing
/// between calls then cannot shift the window.
#[allow(clippy::too_many_arguments)]
fn run_session_axis(
    service: &QueryService,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
    project: Option<String>,
    all: bool,
    axis: SessionAxis,
    include_newest: bool,
    options: MessageQueryOptions,
) -> Result<String> {
    // Fail fast on the age-0 rule (out-of-range needs the scope count and is enforced below).
    if let SessionAxis::Back(age) = &axis
        && let Err(error) = validate_session_back(*age, include_newest)
    {
        return Err(anyhow::Error::new(session_selection_cli_failure(
            error, pretty,
        )?));
    }

    let project_scope = effective_project_scope(project, all);
    let outcome = service.messages_by_session_age(
        project_scope.as_deref(),
        all,
        source_filter,
        &axis,
        include_newest,
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
        let session_ids = response
            .session_selection
            .as_ref()
            .map(|selection| {
                selection
                    .selected
                    .iter()
                    .map(|selected| selected.session_id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        response.next_command = Some(build_next_messages_command(
            cli_source,
            pretty,
            &session_ids,
            None,
            false,
            options.message_index_range,
            options.limit.unwrap_or(0),
            response.next_offset as usize,
            options.sort.by,
            options.sort.order,
        ));
    }

    serialize(&response, pretty)
}

fn recall_with_remotes_response(
    service: &QueryService,
    args: RecallArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
    options: MessageQueryOptions,
) -> Result<String> {
    if let Err(error) = validate_session_back(args.n, args.include_newest) {
        return Err(anyhow::Error::new(session_selection_cli_failure(
            error, pretty,
        )?));
    }

    let project_scope = effective_project_scope(args.project.clone(), args.all);
    let unpaged_options = MessageQueryOptions::new(
        None,
        0,
        SortOptions::new(options.sort.by, options.sort.order),
    );
    let local = service.messages_by_session_age(
        project_scope.as_deref(),
        args.all,
        source_filter,
        &SessionAxis::Back(args.n),
        args.include_newest,
        unpaged_options,
    )?;
    let mut local = match local {
        Ok(response) => response,
        Err(error) => {
            return Err(anyhow::Error::new(session_selection_cli_failure(
                error, pretty,
            )?));
        }
    };

    let project_for_identity = project_identity_input(project_scope.as_deref())?;
    let request = PeerProjectRequest {
        protocol_version: PEER_PROTOCOL_VERSION,
        project: build_peer_project_identity(&project_for_identity),
        source: source_filter,
        all: args.all,
        limits: PeerRequestLimits {
            limit: None,
            offset: 0,
        },
        recall: Some(PeerRecallRequest {
            n: args.n,
            include_newest: args.include_newest,
        }),
    };

    let mut peer_results = Vec::new();
    for remote_name in &args.remotes {
        let mut remote = peer_recall(remote_name, &request, pretty)?;
        let remote_mmr_version = remote
            .peer_results
            .as_ref()
            .and_then(|items| items.first())
            .and_then(|item| item.remote_mmr_version.clone());
        annotate_peer_messages(
            &mut remote.messages,
            remote_name,
            remote_mmr_version.clone(),
        );
        peer_results.push(peer_result_for_messages(
            remote_name,
            "recall",
            remote.total_messages,
            remote_mmr_version,
        ));
        local.messages.extend(remote.messages);
    }

    let session_selection = local.session_selection;
    let mut messages = dedup_api_messages(local.messages);
    sort_api_messages_chronological(&mut messages);
    let total = messages.len() as i64;
    let paged = apply_message_output_pagination(messages, options.limit, options.offset);
    let next_offset = options.offset.saturating_add(paged.len()) as i64;
    let next_page = options.limit.is_some() && next_offset < total;
    let mut response = ApiMessagesResponse {
        messages: paged,
        total_messages: total,
        next_page,
        next_offset,
        next_command: None,
        session_selection,
        peer_results: Some(peer_results),
    };
    if response.next_page {
        response.next_command = Some(build_next_recall_command_with_remotes(
            cli_source,
            pretty,
            project_scope.as_deref(),
            args.all,
            &args.remotes,
            args.n,
            args.include_newest,
            options.limit.unwrap_or(0),
            response.next_offset as usize,
        ));
    }
    serialize(&response, pretty)
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
        SessionSelectionError::SessionRangeOutOfRange {
            total_sessions_in_scope,
            max_selectable_age,
            requested_newest,
            requested_oldest,
        } => {
            value["total_sessions_in_scope"] = serde_json::json!(total_sessions_in_scope);
            value["max_selectable_age"] = serde_json::json!(max_selectable_age);
            value["requested_newest_age"] = serde_json::json!(requested_newest);
            value["requested_oldest_age"] = serde_json::json!(requested_oldest);
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
    let exit_code = if error_kind == "peer_target_invalid"
        || error_kind == "peer_unsupported_format"
        || error_kind == "peer_protocol_error"
    {
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
    fn message_index_range_validation_accepts_open_and_closed_ranges() {
        assert_eq!(validate_message_index_range(None, None).unwrap(), None);
        assert_eq!(
            validate_message_index_range(Some(1), None).unwrap(),
            Some(MessageIndexRange {
                from: Some(1),
                to: None,
            })
        );
        assert_eq!(
            validate_message_index_range(None, Some(3)).unwrap(),
            Some(MessageIndexRange {
                from: None,
                to: Some(3),
            })
        );
        assert_eq!(
            validate_message_index_range(Some(1), Some(3)).unwrap(),
            Some(MessageIndexRange {
                from: Some(1),
                to: Some(3),
            })
        );
    }

    #[test]
    fn message_index_range_validation_rejects_inverted_range() {
        let err = validate_message_index_range(Some(4), Some(1)).unwrap_err();
        assert!(
            err.to_string()
                .contains("--from-message-index must be less than or equal to --to-message-index")
        );
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
    fn parse_session_range_accepts_older_to_newer_span() {
        assert_eq!(parse_session_range("2..1").unwrap(), 1..=2);
        assert_eq!(parse_session_range("1..1").unwrap(), 1..=1);
        assert_eq!(parse_session_range("5..3").unwrap(), 3..=5);
        // Surrounding whitespace is tolerated.
        assert_eq!(parse_session_range(" 2 .. 1 ").unwrap(), 1..=2);
    }

    #[test]
    fn parse_session_range_rejects_reversed_span() {
        let err = parse_session_range("1..2").unwrap_err();
        assert!(
            err.to_string().contains("older..newer"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_session_range_rejects_age_zero_bounds() {
        let err = parse_session_range("1..0").unwrap_err();
        assert!(err.to_string().contains(">= 1"), "unexpected error: {err}");
        assert!(parse_session_range("0..0").is_err());
    }

    #[test]
    fn parse_session_range_rejects_non_numeric_and_negative() {
        assert!(parse_session_range("a..b").is_err());
        assert!(parse_session_range("-1..1").is_err());
        assert!(parse_session_range("2..-1").is_err());
        // Missing separator and empty bounds are rejected.
        assert!(parse_session_range("2.1").is_err());
        assert!(parse_session_range("..1").is_err());
        assert!(parse_session_range("2..").is_err());
        // A trailing extra bound is not a valid integer.
        assert!(parse_session_range("3..2..1").is_err());
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
