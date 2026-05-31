use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::agent::ai;
use crate::capture::{
    ClaudeAdapter, CodexAdapter, CursorAdapter, Reconciler, SourceAdapter, SourceDiscoveryRoot,
};
use crate::dream::{
    DreamEvidence, DreamEvidenceMode, OmittedDreamEvidence, build_evidence_bundle,
    build_source_evidence_bundle,
};
use crate::messages::service::{
    MessageIndexRange, MessageQueryOptions, QueryService, SessionAxis, SessionSelectionError,
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
    ApplyOptions, ExportOptions, InspectOptions, PackOptions, ReadOptions, ReceiveOptions,
    ResumeOptions, SendOptions, SendTransport, ServeError, ServeOptions, TeleportFailure,
    TeleportFidelity, TeleportOutputFormat, TeleportStatus, apply_bundle, export_bundle,
    inspect_bundle, pack_session, parse_export_as, parse_resume_agent_as, read_bundle,
    receive_bundle, resolve_bundle_locator, resolve_read_locator, resolve_receive_locator,
    resume_bundle, send_session, serve_session,
};
use crate::types::{
    Agent, ApiMessage, ApiMessagesResponse, RememberRequest, RememberResponse, RememberSelection,
    SortBy, SortOptions, SortOrder, SourceFilter,
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
const ENV_DEFAULT_REMEMBER_AGENT: &str = "MMR_DEFAULT_REMEMBER_AGENT";
const ENV_DEFAULT_SOURCE: &str = "MMR_DEFAULT_SOURCE";

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
    about = "Browse AI conversation history from Claude, Codex, Cursor, Grok, and Pi",
    after_help = "Examples:\n  mmr init\n  mmr status --pretty\n  mmr list projects --pretty\n  mmr list sessions --project /path/to/project\n  mmr recall --pretty\n  mmr read session <session-id> --pretty\n  mmr read project --format tree --output-dir /tmp/mmr-tree\n  mmr find \"migration append-only\" --format line\n  mmr summarize project --project /path/to/project\n  mmr assimilate project --pretty\n  mmr skill load\n  mmr skill install --local\n  mmr sync --pretty\n\nOutput:\n  Commands emit machine-readable JSON on stdout unless an explicit stream format such as --format line is selected. Use --pretty for indented JSON."
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
    /// Retrieve a previous stable session for immediate continuity
    Recall(RecallArgs),
    /// Read raw session, project, or source history
    Read(ReadArgs),
    /// Produce scoped context for future agents
    Context(ContextArgs),
    /// Run a stateless summary over scoped history
    Summarize(SummarizeArgs),
    /// Return prompt/runbook/evidence for memory assimilation
    Assimilate(AssimilateArgs),
    /// Load or install the bundled mmr agent skill
    Skill(SkillArgs),
    /// Import normalized events into the local memory store
    Import(ImportArgs),
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
    /// Move a selected coding-agent session between machines
    Teleport(TeleportArgs),
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
}

#[derive(Args, Debug)]
pub struct ContextSourceArgs {
    /// Maximum number of recent messages to include
    #[arg(long, default_value_t = 200)]
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
    #[command(flatten)]
    runner: SummaryRunnerArgs,
}

#[derive(Args, Debug)]
pub struct SummarizeSourceArgs {
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
    #[command(flatten)]
    runner: SummaryRunnerArgs,
}

#[derive(Args, Debug, Clone)]
pub struct SummaryRunnerArgs {
    /// Agent to use (defaults to MMR_DEFAULT_REMEMBER_AGENT or cursor / composer-2-fast)
    #[arg(long, value_enum)]
    agent: Option<Agent>,
    /// Override the output format and rules portion of the system instructions
    #[arg(long)]
    instructions: Option<String>,
    /// Output format for summary results
    #[arg(short = 'O', long = "output-format", value_enum, default_value = "md")]
    output_format: RememberOutputFormatArg,
    /// Model to use
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
pub enum RememberOutputFormatArg {
    Json,
    #[default]
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
pub struct ImportArgs {
    /// Project path to link/import into
    #[arg(long)]
    project: PathBuf,
    /// Source root (defaults to $HOME/.codex, $HOME/.claude, or $HOME/.cursor based on --source)
    #[arg(long = "source-root")]
    source_root: Option<PathBuf>,
}

#[derive(Args, Debug)]
#[command(
    after_help = "Current release: native session handoff for codex, claude, cursor, grok, and pi (not mmr sync).\n\
See docs/mmr-teleport.md for provider matrix and workflows.\n\
Examples:\n  \
mmr teleport serve --source grok --session sess-abc\n  \
mmr teleport read mmtp://100.x.x.x:PORT/TOKEN\n  \
mmr teleport receive mmtp://100.x.x.x:PORT/TOKEN\n  \
mmr teleport send --source claude --session sess-abc --to user@host\n  \
mmr teleport send --session sess-abc --to file:///path/to/inbox"
)]
pub struct TeleportArgs {
    #[command(subcommand)]
    command: TeleportCommand,
}

#[derive(Subcommand, Debug)]
pub enum TeleportCommand {
    /// Pack a native provider session into a .mmr bundle (local handoff artifact)
    Pack(TeleportPackArgs),
    /// Validate bundle hashes and manifest without writing agent files
    Inspect(TeleportInspectArgs),
    /// Install a native provider bundle into the target agent storage layout
    Apply(TeleportApplyArgs),
    /// Pack and transfer a session over SSH (user@host) or file:// inbox
    Send(TeleportSendArgs),
    /// Download or cache a bundle for read-only access (no native apply)
    Read(TeleportReadArgs),
    /// Download mmtp:// URL or apply a local bundle / ready inbox entry
    Receive(TeleportReceiveArgs),
    /// Apply a bundle and report provider resume steps (--as same|codex|claude|cursor|grok|pi)
    Resume(TeleportResumeArgs),
    /// Write native artifact(s) from a bundle (--as same|<provider>)
    Export(TeleportExportArgs),
    /// Pack one session and serve it on a one-shot mmtp:// URL until downloaded
    Serve(TeleportServeArgs),
}

#[derive(Args, Debug)]
#[command(
    after_help = "Native bundles for codex, claude, cursor, grok, and pi; stderr warns that bundles may contain secrets.\n\
Example: mmr teleport pack --source codex --session sess-abc --to ./handoff.mmr"
)]
pub struct TeleportPackArgs {
    /// Session ID to pack (omit for latest session in scope; default source is codex)
    #[arg(long)]
    session: Option<String>,
    /// Select the latest session in scope (default when --session is omitted)
    #[arg(long)]
    latest: bool,
    /// Project name or path
    #[arg(long)]
    project: Option<String>,
    /// Output path for the bundle artifact
    #[arg(long)]
    to: Option<PathBuf>,
    /// Bundle fidelity (default: native; only native bundles are supported).
    #[arg(long = "as")]
    fidelity: Option<String>,
    /// Show what would be packed without writing a bundle
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args, Debug)]
pub struct TeleportInspectArgs {
    /// Bundle path to inspect
    #[arg(value_name = "BUNDLE_PATH")]
    bundle_path: Option<PathBuf>,
    /// Bundle path or inbox directory to read
    #[arg(long)]
    to: Option<PathBuf>,
    /// Output format
    #[arg(
        short = 'O',
        long = "output-format",
        value_enum,
        default_value = "json"
    )]
    output_format: TeleportOutputFormatArg,
    /// Include extra manifest diagnostics
    #[arg(long)]
    verbose: bool,
    /// Not valid for inspect; use -O for output format
    #[arg(long = "as", hide = true)]
    as_flag: Option<String>,
}

#[derive(Args, Debug)]
pub struct TeleportApplyArgs {
    /// Bundle path to apply
    #[arg(value_name = "BUNDLE_PATH")]
    bundle_path: Option<PathBuf>,
    /// Bundle path or inbox directory to read
    #[arg(long)]
    to: Option<PathBuf>,
    /// Target project path override
    #[arg(long)]
    project: Option<String>,
    /// Show what would be applied without writing files
    #[arg(long)]
    dry_run: bool,
    /// Replace existing native files and re-import store events
    #[arg(long)]
    force: bool,
    /// Skip importing normalized events into the linked mmr store
    #[arg(long)]
    skip_store_import: bool,
    /// Not valid for apply; use -O for output format
    #[arg(long = "as", hide = true)]
    as_flag: Option<String>,
}

#[derive(Args, Debug)]
#[command(after_help = "Examples:\n  \
mmr teleport send --session sess-abc --to user@host\n  \
mmr teleport send --session sess-abc --to file:///Users/me/Sync/mmr-inbox\n\
HTTP one-shot URLs use `teleport serve`, not send.")]
pub struct TeleportSendArgs {
    /// Session ID to send (omit for latest session in scope; default source is codex)
    #[arg(long)]
    session: Option<String>,
    /// Select the latest session in scope (default when --session is omitted)
    #[arg(long)]
    latest: bool,
    /// Project name or path
    #[arg(long)]
    project: Option<String>,
    /// SSH destination (user@host) or file inbox directory (file:///path/to/inbox)
    #[arg(long)]
    to: Option<String>,
    /// Transport selector (default: auto; inferred from --to)
    #[arg(long)]
    transport: Option<TeleportTransportArg>,
    /// Show planned transport steps without writing files or contacting remote hosts
    #[arg(long)]
    dry_run: bool,
    /// Not valid for send; native fidelity only
    #[arg(long = "as", hide = true)]
    as_flag: Option<String>,
}

#[derive(Args, Debug)]
#[command(after_help = "Examples:\n  \
mmr teleport read mmtp://100.x.x.x:8765/TOKEN\n  \
mmr teleport read ./handoff.mmr\n  \
mmr teleport read ~/.mmr/teleport/cache/tp:v1:.../bundle.mmr\n  \
mmr teleport read ./handoff.mmr -O md\n\
Caches under ~/.mmr/teleport/cache/<bundle_id>/ and prints session messages on stdout.")]
pub struct TeleportReadArgs {
    /// Bundle path, inbox directory, or HTTP locator (mmtp://host:port/token)
    #[arg(value_name = "LOCATOR")]
    bundle_path: Option<String>,
    /// Bundle path, inbox directory, or HTTP locator (mmtp://host:port/token)
    #[arg(long)]
    to: Option<String>,
    /// Output format
    #[arg(
        short = 'O',
        long = "output-format",
        value_enum,
        default_value = "json"
    )]
    output_format: TeleportOutputFormatArg,
    /// Show what would be cached without writing files or downloading
    #[arg(long)]
    dry_run: bool,
    /// Not valid for read; use -O for output format
    #[arg(long = "as", hide = true)]
    as_flag: Option<String>,
}

#[derive(Args, Debug)]
#[command(after_help = "Examples:\n  \
mmr teleport receive mmtp://100.x.x.x:8765/TOKEN\n  \
mmr teleport receive ./handoff.mmr --project /path/to/project\n  \
mmr teleport receive --to ~/.mmr/teleport/inbox/tp:v1:...\n\
Incomplete inbox entries (no ready marker) return ok with empty staged.")]
pub struct TeleportReceiveArgs {
    /// Bundle path, inbox directory, or HTTP locator (mmtp://host:port/token)
    #[arg(value_name = "LOCATOR")]
    bundle_path: Option<String>,
    /// Bundle path, inbox directory, or HTTP locator (mmtp://host:port/token)
    #[arg(long)]
    to: Option<String>,
    /// Show what would be received without applying
    #[arg(long)]
    dry_run: bool,
    /// Target project path override for apply
    #[arg(long)]
    project: Option<String>,
    /// Replace existing native files when applying
    #[arg(long)]
    force: bool,
    /// Not valid for receive; use -O for output format
    #[arg(long = "as", hide = true)]
    as_flag: Option<String>,
}

#[derive(Args, Debug)]
#[command(after_help = "Examples:\n  \
mmr teleport resume ./handoff.mmr --project /path/to/project\n  \
mmr teleport resume ./handoff.mmr --as same --no-agent-exec\n\
Cross-agent --as values return status unsupported (exit 3).")]
pub struct TeleportResumeArgs {
    /// Bundle path to resume
    #[arg(value_name = "REF")]
    bundle_path: Option<PathBuf>,
    /// Bundle path or inbox directory to read
    #[arg(long)]
    to: Option<PathBuf>,
    /// Target agent (--as same uses bundle source; cross-agent may return unsupported)
    #[arg(long = "as")]
    agent: Option<String>,
    /// Target project path override
    #[arg(long)]
    project: Option<String>,
    /// Show what would be applied without writing files
    #[arg(long)]
    dry_run: bool,
    /// Replace existing native files when applying
    #[arg(long)]
    force: bool,
    /// Do not invoke the provider resume CLI after apply
    #[arg(long)]
    no_agent_exec: bool,
}

#[derive(Args, Debug)]
#[command(after_help = "Examples:\n  \
mmr teleport export ./handoff.mmr --to ./out.jsonl --as same\n  \
mmr teleport export ./grok.mmr --to ./grok-export-dir --as grok\n\
Distinct from top-level `mmr read project` (local history query).")]
pub struct TeleportExportArgs {
    /// Bundle path or inbox locator to export
    #[arg(value_name = "REF")]
    bundle_path: Option<PathBuf>,
    /// Destination path for the exported artifact
    #[arg(long)]
    to: Option<PathBuf>,
    /// Output representation (--as same|codex|claude|cursor|grok|pi; cross-agent returns unsupported exit 3)
    #[arg(long = "as")]
    format: Option<String>,
    /// Show what would be exported without writing files
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args, Debug)]
#[command(
    after_help = "Prints one startup JSON object (listen_url, token, expires_at) then blocks.\n\
Example: mmr teleport serve --session sess-abc --bind 100.x.x.x:0\n\
Reader: mmr teleport read mmtp://100.x.x.x:PORT/TOKEN\n\
Receiver (handoff): mmr teleport receive mmtp://100.x.x.x:PORT/TOKEN"
)]
pub struct TeleportServeArgs {
    /// Session ID to serve (omit for latest session in scope; default source is codex)
    #[arg(long)]
    session: Option<String>,
    /// Select the latest session in scope (default when --session is omitted)
    #[arg(long)]
    latest: bool,
    /// Project name or path
    #[arg(long)]
    project: Option<String>,
    /// Bind address host:port (alias for --bind)
    #[arg(long)]
    to: Option<String>,
    /// Bind address host:port
    #[arg(long)]
    bind: Option<String>,
    /// Seconds to wait for one successful download before exiting
    #[arg(long, default_value_t = 600)]
    timeout: u64,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "kebab-case")]
enum TeleportTransportArg {
    Auto,
    Ssh,
    Http,
    File,
}

impl From<TeleportTransportArg> for SendTransport {
    fn from(value: TeleportTransportArg) -> Self {
        match value {
            TeleportTransportArg::Auto => SendTransport::Auto,
            TeleportTransportArg::Ssh => SendTransport::Ssh,
            TeleportTransportArg::File => SendTransport::File,
            TeleportTransportArg::Http => SendTransport::Auto,
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
enum TeleportOutputFormatArg {
    #[default]
    Json,
    Md,
}

impl From<TeleportOutputFormatArg> for TeleportOutputFormat {
    fn from(value: TeleportOutputFormatArg) -> Self {
        match value {
            TeleportOutputFormatArg::Json => TeleportOutputFormat::Json,
            TeleportOutputFormatArg::Md => TeleportOutputFormat::Md,
        }
    }
}

pub async fn run_cli(cli: Cli) -> Result<String> {
    let source_filter = effective_source(cli.source);
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
        return serialize(&import_response(args, source_filter)?, cli.pretty);
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
    if let Commands::Teleport(args) = &cli.command {
        return teleport_command_response(args, source_filter, cli.pretty);
    }

    let service = QueryService::load()?;

    let response = match cli.command {
        Commands::List(args) => list_command_response(&service, args, source_filter, cli.pretty)?,
        Commands::Recall(args) => {
            let options = MessageQueryOptions::new(
                Some(args.limit),
                0,
                SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
            );
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
        Commands::Import(args) => serialize(&import_response(&args, source_filter)?, cli.pretty)?,
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
        Commands::DbInfo {
            project,
            smoke_event,
        } => serialize(&db_info_response(project, smoke_event)?, cli.pretty)?,
        Commands::Teleport(_) => unreachable!("teleport handled before QueryService load"),
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
    })
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

fn context_command_response(
    service: &QueryService,
    args: ContextArgs,
    cli_source: Option<SourceFilter>,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match args.command {
        ContextCommand::Project(args) => {
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
                },
                pretty,
            )
        }
        ContextCommand::Source(args) => {
            let source = require_explicit_source(cli_source, "context source")?;
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
                },
                pretty,
            )
        }
    }
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
            let response = ai::remember(
                service,
                RememberRequest {
                    agent: effective_remember_agent(args.runner.agent),
                    project: project.as_str(),
                    selection: RememberSelection::All,
                    source: source_filter,
                    instructions: args.runner.instructions.as_deref(),
                    model: args.runner.model.as_deref(),
                },
            )
            .await?;
            format_remember_response(&response, args.runner.output_format, pretty)
        }
        SummarizeCommand::Session(args) => {
            let messages = service.messages(
                &[args.session_id],
                args.project.as_deref(),
                source_filter,
                MessageQueryOptions::new(
                    None,
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )?;
            let formatted = format_messages_as_summary_input(&messages.messages);
            let response = ai::summarize_formatted_messages(
                effective_remember_agent(args.runner.agent),
                args.runner.instructions.as_deref(),
                args.runner.model.as_deref(),
                &formatted,
            )
            .await?;
            format_remember_response(&response, args.runner.output_format, pretty)
        }
        SummarizeCommand::Source(args) => {
            let source = require_explicit_source(cli_source, "summarize source")?;
            let messages = service.messages(
                &[],
                None,
                Some(source),
                MessageQueryOptions::new(
                    None,
                    0,
                    SortOptions::new(SortBy::Timestamp, SortOrder::Asc),
                ),
            )?;
            let formatted = format_messages_as_summary_input(&messages.messages);
            let response = ai::summarize_formatted_messages(
                effective_remember_agent(args.runner.agent),
                args.runner.instructions.as_deref(),
                args.runner.model.as_deref(),
                &formatted,
            )
            .await?;
            format_remember_response(&response, args.runner.output_format, pretty)
        }
    }
}

fn format_messages_as_summary_input(messages: &[ApiMessage]) -> String {
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

fn teleport_command_response(
    args: &TeleportArgs,
    source_filter: Option<SourceFilter>,
    pretty: bool,
) -> Result<String> {
    match &args.command {
        TeleportCommand::Pack(pack) => {
            if pack.session.is_some() && pack.latest {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/pack",
                        "pass either --session or --latest, not both",
                    ),
                    pretty,
                );
            }
            let fidelity = match parse_pack_fidelity(pack.fidelity.as_deref()) {
                Ok(fidelity) => fidelity,
                Err(failure) => return teleport_fail(failure, pretty),
            };
            let service = QueryService::load()?;
            let project = pack.project.clone().or_else(|| {
                if pack.session.is_some() {
                    None
                } else {
                    effective_project_scope(None, false)
                }
            });
            match pack_session(
                &service,
                PackOptions {
                    session_id: pack.session.clone(),
                    project,
                    source_filter,
                    output_path: pack.to.clone(),
                    fidelity,
                    dry_run: pack.dry_run,
                },
            ) {
                Ok(response) => serialize(&response, pretty),
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Inspect(inspect) => {
            if inspect.as_flag.is_some() {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/inspect",
                        "--as is not valid for teleport inspect; use -O for output format",
                    ),
                    pretty,
                );
            }
            let bundle_path = match resolve_bundle_locator(
                inspect.bundle_path.clone(),
                inspect.to.clone(),
                "inspect",
            ) {
                Ok(path) => path,
                Err(error) => return teleport_fail(error.into(), pretty),
            };
            match inspect_bundle(InspectOptions {
                bundle_path,
                output_format: inspect.output_format.into(),
                verbose: inspect.verbose,
            }) {
                Ok(response) => serialize(&response, pretty),
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Apply(apply) => {
            if apply.as_flag.is_some() {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/apply",
                        "--as is not valid for teleport apply; use -O for output format",
                    ),
                    pretty,
                );
            }
            let bundle_path = match resolve_bundle_locator(
                apply.bundle_path.clone(),
                apply.to.clone(),
                "apply",
            ) {
                Ok(path) => path,
                Err(error) => return teleport_fail(error.into(), pretty),
            };
            match apply_bundle(ApplyOptions {
                bundle_path,
                project: apply.project.clone(),
                dry_run: apply.dry_run,
                force: apply.force,
                skip_store_import: apply.skip_store_import,
            }) {
                Ok(response) => serialize(&response, pretty),
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Send(send) => {
            if send.as_flag.is_some() {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/send",
                        "--as is not valid for teleport send in this release; native fidelity is always used",
                    ),
                    pretty,
                );
            }
            if send.session.is_some() && send.latest {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/send",
                        "pass either --session or --latest, not both",
                    ),
                    pretty,
                );
            }
            let to = match send.to.clone() {
                Some(to) => to,
                None => {
                    return teleport_fail(
                        TeleportFailure::usage(
                            "teleport/send",
                            "--to is required for teleport send",
                        ),
                        pretty,
                    );
                }
            };
            let transport = match send.transport {
                Some(TeleportTransportArg::Http) => {
                    return teleport_fail(
                        TeleportFailure::usage(
                            "teleport/send",
                            "teleport send does not support HTTP transport yet",
                        ),
                        pretty,
                    );
                }
                Some(TeleportTransportArg::Auto) => SendTransport::Auto,
                Some(TeleportTransportArg::Ssh) => SendTransport::Ssh,
                Some(TeleportTransportArg::File) => SendTransport::File,
                None => SendTransport::from_env_or_default(),
            };
            let service = QueryService::load()?;
            let project = send.project.clone().or_else(|| {
                if send.session.is_some() {
                    None
                } else {
                    effective_project_scope(None, false)
                }
            });
            match send_session(
                &service,
                SendOptions {
                    session_id: send.session.clone(),
                    project,
                    source_filter,
                    to,
                    transport,
                    dry_run: send.dry_run,
                },
            ) {
                Ok(response) => {
                    let json = serialize(&response, pretty)?;
                    if response.status == TeleportStatus::Partial {
                        return Err(anyhow::Error::new(CliFailure::new(
                            3,
                            json,
                            "teleport: remote mmr missing; bundle staged in remote inbox",
                        )));
                    }
                    Ok(json)
                }
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Read(read) => {
            if read.as_flag.is_some() {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/read",
                        "--as is not valid for teleport read; use -O for output format",
                    ),
                    pretty,
                );
            }
            let locator = match resolve_read_locator(read.bundle_path.clone(), read.to.clone()) {
                Ok(path) => path,
                Err(failure) => return teleport_fail(failure, pretty),
            };
            match read_bundle(ReadOptions {
                locator,
                dry_run: read.dry_run,
                output_format: read.output_format.into(),
            }) {
                Ok(response) => serialize(&response, pretty),
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Receive(receive) => {
            if receive.as_flag.is_some() {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/receive",
                        "--as is not valid for teleport receive; use -O for output format",
                    ),
                    pretty,
                );
            }
            let locator =
                match resolve_receive_locator(receive.bundle_path.clone(), receive.to.clone()) {
                    Ok(path) => path,
                    Err(failure) => return teleport_fail(failure, pretty),
                };
            match receive_bundle(ReceiveOptions {
                locator,
                dry_run: receive.dry_run,
                project: receive.project.clone(),
                force: receive.force,
            }) {
                Ok(response) => serialize(&response, pretty),
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Resume(resume) => {
            let (requested_as, requested_as_label) =
                match parse_resume_agent_as(resume.agent.as_deref()) {
                    Ok(parsed) => parsed,
                    Err(failure) => return teleport_fail(failure, pretty),
                };
            let bundle_path = match resolve_bundle_locator(
                resume.bundle_path.clone(),
                resume.to.clone(),
                "resume",
            ) {
                Ok(path) => path,
                Err(error) => return teleport_fail(error.into(), pretty),
            };
            match resume_bundle(ResumeOptions {
                bundle_path,
                project: resume.project.clone(),
                dry_run: resume.dry_run,
                force: resume.force,
                no_agent_exec: resume.no_agent_exec,
                requested_as,
                requested_as_label,
            }) {
                Ok(response) => teleport_success_or_unsupported(response, pretty),
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Export(export) => {
            let bundle_path = match export.bundle_path.clone() {
                Some(path) => path,
                None => {
                    return teleport_fail(
                        TeleportFailure::usage(
                            "teleport/export",
                            "teleport export: bundle ref is required as a positional argument",
                        ),
                        pretty,
                    );
                }
            };
            let to = match export.to.clone() {
                Some(path) => path,
                None => {
                    return teleport_fail(
                        TeleportFailure::usage(
                            "teleport/export",
                            "--to is required for teleport export",
                        ),
                        pretty,
                    );
                }
            };
            let (requested_as, requested_as_label) = match parse_export_as(export.format.as_deref())
            {
                Ok(parsed) => parsed,
                Err(failure) => return teleport_fail(failure, pretty),
            };
            match export_bundle(ExportOptions {
                bundle_path,
                to,
                requested_as,
                requested_as_label,
                dry_run: export.dry_run,
            }) {
                Ok(response) => teleport_success_or_unsupported(response, pretty),
                Err(failure) => teleport_fail(failure, pretty),
            }
        }
        TeleportCommand::Serve(serve) => {
            if serve.session.is_some() && serve.latest {
                return teleport_fail(
                    TeleportFailure::usage(
                        "teleport/serve",
                        "pass either --session or --latest, not both",
                    ),
                    pretty,
                );
            }
            let service = QueryService::load()?;
            let project = serve.project.clone().or_else(|| {
                if serve.session.is_some() {
                    None
                } else {
                    effective_project_scope(None, false)
                }
            });
            let bind = serve.bind.clone().or(serve.to.clone());
            match serve_session(
                &service,
                ServeOptions {
                    session_id: serve.session.clone(),
                    project,
                    source_filter,
                    bind,
                    timeout_secs: serve.timeout,
                },
            ) {
                Ok(()) => Ok(String::new()),
                Err(ServeError::BeforeStartup(failure)) => teleport_fail(failure, pretty),
                Err(ServeError::TimedOut) => Err(anyhow::Error::new(CliFailure::new(
                    3,
                    String::new(),
                    "teleport serve: timed out waiting for bundle download",
                ))),
            }
        }
    }
}

fn teleport_fail(failure: TeleportFailure, pretty: bool) -> Result<String> {
    Err(anyhow::Error::new(CliFailure::from_teleport(
        failure, pretty,
    )?))
}

fn teleport_success_or_unsupported<T: Serialize>(response: T, pretty: bool) -> Result<String> {
    let status = serde_json::to_value(&response).ok().and_then(|value| {
        value
            .get("status")
            .and_then(|status| status.as_str())
            .map(str::to_string)
    });
    let json = serialize(&response, pretty)?;
    if status.as_deref() == Some("unsupported") {
        let stderr = response_message(&response).unwrap_or_else(|| {
            "teleport: requested cross-agent transform is not supported".to_string()
        });
        return Err(anyhow::Error::new(CliFailure::new(3, json, stderr)));
    }
    Ok(json)
}

fn response_message<T: Serialize>(response: &T) -> Option<String> {
    serde_json::to_value(response)
        .ok()?
        .get("message")?
        .as_str()
        .map(str::to_string)
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

fn parse_pack_fidelity(as_flag: Option<&str>) -> Result<TeleportFidelity, TeleportFailure> {
    match as_flag {
        None | Some("") | Some("native") => Ok(TeleportFidelity::Native),
        Some("shared-safe") => Err(TeleportFailure::runtime(
            "teleport/pack",
            "teleport pack supports native bundles only; --as shared-safe is not supported",
        )),
        Some(other) => Err(TeleportFailure::usage(
            "teleport/pack",
            format!(
                "unsupported --as value {other:?}; teleport pack supports native fidelity only (use --as native or omit --as)"
            ),
        )),
    }
}

#[derive(Debug, Serialize)]
struct NoteResponse {
    project_id: String,
    event_id: String,
    source: String,
    citation: String,
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
    agent: Agent,
    status: String,
    command: Option<String>,
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
            "mmr import --source {}",
            source_filter_name(Some(source)).expect("source name")
        )],
        None => ["codex", "claude", "cursor"]
            .iter()
            .map(|source| format!("mmr import --source {source}"))
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
                "Set HOME or run `mmr --source {source_name} import --project {} --source-root <source-root>`.",
                shell_quote_path(project_path)
            )),
        });
    };
    let source_root_text = source_root.display().to_string();
    if source_root.exists() {
        let action = (event_count == 0).then(|| {
            format!(
                "Run `mmr --source {source_name} import --project {} --source-root {}` to reconcile matching sessions.",
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
                "Run the {source_name} provider once, or run `mmr --source {source_name} import --project {} --source-root <source-root>` with the correct source root.",
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
    let agent = effective_remember_agent(None);
    match agent {
        Agent::Gemini => {
            let configured =
                env_has_non_empty("GOOGLE_API_KEY") || env_has_non_empty("GEMINI_API_KEY");
            StatusSummaryRunnerDiagnostic {
                agent,
                status: if configured {
                    "configured"
                } else {
                    "missing_api_key"
                }
                .to_string(),
                command: None,
                api_key_env: vec!["GOOGLE_API_KEY".to_string(), "GEMINI_API_KEY".to_string()],
                action: (!configured).then(|| {
                    "Set GOOGLE_API_KEY or GEMINI_API_KEY; for status readiness with another backend set MMR_DEFAULT_REMEMBER_AGENT=cursor|codex, or run one brief with `mmr summarize project --agent cursor|codex`."
                        .to_string()
                }),
            }
        }
        Agent::Cursor => {
            let api_key_configured = env_has_non_empty("CURSOR_API_KEY");
            let command_available = command_on_path("agent");
            let (status, action) = if !api_key_configured {
                (
                    "missing_api_key",
                    Some(
                        "Set CURSOR_API_KEY; for status readiness with another backend set MMR_DEFAULT_REMEMBER_AGENT=gemini|codex, or run one brief with `mmr summarize project --agent gemini|codex`."
                            .to_string(),
                    ),
                )
            } else if !command_available {
                (
                    "missing_command",
                    Some("Install the Cursor `agent` CLI on PATH; for status readiness with another backend set MMR_DEFAULT_REMEMBER_AGENT=gemini|codex, or run one brief with `mmr summarize project --agent gemini|codex`.".to_string()),
                )
            } else {
                ("configured", None)
            };
            StatusSummaryRunnerDiagnostic {
                agent,
                status: status.to_string(),
                command: Some("agent".to_string()),
                api_key_env: vec!["CURSOR_API_KEY".to_string()],
                action,
            }
        }
        Agent::Codex => {
            let command_available = command_on_path("codex");
            StatusSummaryRunnerDiagnostic {
                agent,
                status: if command_available {
                    "configured"
                } else {
                    "missing_command"
                }
                .to_string(),
                command: Some("codex".to_string()),
                api_key_env: Vec::new(),
                action: (!command_available).then(|| {
                    "Install the Codex CLI on PATH; for status readiness with another backend set MMR_DEFAULT_REMEMBER_AGENT=cursor|gemini, or run one brief with `mmr summarize project --agent cursor|gemini`."
                        .to_string()
                }),
            }
        }
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

fn env_has_non_empty(name: &str) -> bool {
    std::env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn command_on_path(command: &str) -> bool {
    if command.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(command).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|path| path.join(command).is_file()))
        .unwrap_or(false)
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
struct ImportResponse {
    project_id: String,
    source: String,
    discovered_sessions: usize,
    imported_events: usize,
    warnings: Vec<String>,
    event_ids: Vec<String>,
}

fn import_response(
    args: &ImportArgs,
    source_filter: Option<SourceFilter>,
) -> Result<ImportResponse> {
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
            "`mmr import` currently requires `--source codex`, `--source claude`, or `--source cursor`"
        ),
    };

    Ok(ImportResponse {
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
            "`mmr import` currently requires `--source codex`, `--source claude`, or `--source cursor`"
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

fn effective_remember_agent(cli_agent: Option<Agent>) -> Agent {
    cli_agent
        .or_else(default_remember_agent_from_env)
        .unwrap_or(Agent::Cursor)
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

fn default_remember_agent_from_env() -> Option<Agent> {
    std::env::var(ENV_DEFAULT_REMEMBER_AGENT)
        .ok()
        .and_then(|value| parse_agent_env(&value))
}

fn parse_agent_env(value: &str) -> Option<Agent> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "codex" => Some(Agent::Codex),
        "cursor" => Some(Agent::Cursor),
        "gemini" => Some(Agent::Gemini),
        _ => None,
    }
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

fn remember_response_to_markdown(response: &RememberResponse) -> String {
    if response.text.trim().is_empty() {
        "(No continuity brief returned.)"
    } else {
        response.text.trim()
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
            "--agent",
            "gemini",
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
        assert_eq!(project.runner.agent, Some(Agent::Gemini));
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
        assert_eq!(session.runner.agent, None);
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
    fn parse_agent_env_accepts_supported_values() {
        assert_eq!(parse_agent_env("codex"), Some(Agent::Codex));
        assert_eq!(parse_agent_env("cursor"), Some(Agent::Cursor));
        assert_eq!(parse_agent_env("CURSOR"), Some(Agent::Cursor));
        assert_eq!(parse_agent_env("GEMINI"), Some(Agent::Gemini));
        assert_eq!(parse_agent_env(""), None);
        assert_eq!(parse_agent_env("invalid"), None);
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
            agent: Agent::Gemini,
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
            agent: Agent::Gemini,
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
            agent: Agent::Gemini,
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
    fn import_command_parses_with_global_source_after_subcommand() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "import",
            "--source",
            "codex",
            "--project",
            "/tmp/project",
            "--source-root",
            "/tmp/.codex",
        ])
        .expect("import should parse");
        assert_eq!(parsed.source, Some(SourceFilter::Codex));
        let Commands::Import(args) = parsed.command else {
            panic!("expected import command");
        };
        assert_eq!(args.project, PathBuf::from("/tmp/project"));
        assert_eq!(args.source_root, Some(PathBuf::from("/tmp/.codex")));
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
