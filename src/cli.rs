use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::agent::ai;
use crate::messages::service::{MessageIndexRange, MessageQueryOptions, QueryService};
use crate::redaction::{
    PiiCoverage, PiiCoverageStatus, RedactionFinding, RedactionOutcome, scan_text,
};
use crate::store::{
    DEFAULT_REDACTION_POLICY_ID, EventRecord, NewEvent, NewRedactionSpan, ProjectRecord, Store,
    content_hash,
};
use crate::types::{
    Agent, ApiMessage, ApiMessagesResponse, RememberRequest, RememberResponse, RememberSelection,
    SortBy, SortOptions, SortOrder, SourceFilter,
};

const ENV_AUTO_DISCOVER_PROJECT: &str = "MMR_AUTO_DISCOVER_PROJECT";
const ENV_DEFAULT_REMEMBER_AGENT: &str = "MMR_DEFAULT_REMEMBER_AGENT";
const ENV_DEFAULT_SOURCE: &str = "MMR_DEFAULT_SOURCE";

#[derive(Parser, Debug)]
#[command(
    name = "mmr",
    about = "Browse AI conversation history from Claude, Codex, Cursor, Grok, and Pi"
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
    /// List projects
    Projects {
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
    },
    /// List sessions (defaults to the current project when cwd auto-discovery succeeds)
    Sessions {
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
    },
    /// Get messages (defaults to the current project when cwd auto-discovery succeeds)
    Messages {
        /// Session ID
        #[arg(long)]
        session: Option<String>,
        /// Project name or path
        #[arg(long)]
        project: Option<String>,
        /// Return messages across all projects instead of the auto-discovered cwd project
        #[arg(long)]
        all: bool,
        /// Return the newest N messages from the latest session in scope
        #[arg(long, num_args = 0..=1, default_missing_value = "1")]
        latest: Option<NonZeroUsize>,
        /// First zero-based message index to include after filtering and sorting
        #[arg(long)]
        from_message_index: Option<usize>,
        /// Zero-based message index to stop before after filtering and sorting
        #[arg(long)]
        to_message_index: Option<usize>,
        /// Return only the latest N messages
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Number of sorted messages to skip
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Sort messages by
        #[arg(short = 's', long, default_value = "timestamp")]
        sort_by: SortBy,
        /// Sort order: asc or desc
        #[arg(short = 'o', long, default_value = "asc")]
        order: SortOrder,
    },
    /// All messages for the current project (cwd) or --project, all sources, chronological
    Export {
        /// Project name or path (omit to use current directory)
        #[arg(long)]
        project: Option<String>,
    },
    /// Generate a stateless continuity brief from prior sessions
    Remember(RememberArgs),
    /// Add a human-authored note to the local memory store
    Note {
        /// Note text. Omit to read multiline text from stdin.
        #[arg(value_name = "TEXT", trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Inspect and apply local redaction policy before sync
    Redact(RedactArgs),
    /// Sync safety view; full remote sync lands in NHL-277
    Sync(SyncArgs),
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
}

#[derive(Args, Debug)]
pub struct RememberArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long, short = 'p', global = true)]
    project: Option<String>,
    /// Agent to use (defaults to MMR_DEFAULT_REMEMBER_AGENT or cursor / composer-2-fast)
    #[arg(long, value_enum, global = true)]
    agent: Option<Agent>,
    /// Override the output format and rules portion of the system instructions
    #[arg(long, global = true)]
    instructions: Option<String>,
    /// Output format for remember results
    #[arg(
        short = 'O',
        long = "output-format",
        value_enum,
        default_value = "md",
        global = true
    )]
    output_format: RememberOutputFormatArg,
    /// Gemini model to use
    #[arg(long, global = true)]
    model: Option<String>,
    /// Session selector (omit for latest)
    #[command(subcommand)]
    selection: Option<RememberSelectorCommand>,
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

impl RememberArgs {
    fn selection(&self) -> RememberSelection {
        match &self.selection {
            None => RememberSelection::Latest,
            Some(RememberSelectorCommand::All) => RememberSelection::All,
            Some(RememberSelectorCommand::Session { session_id }) => RememberSelection::Session {
                session_id: session_id.clone(),
            },
        }
    }
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum RememberSelectorCommand {
    /// Use all matching sessions
    All,
    /// Use one specific session
    Session {
        /// Session ID to use for the remember operation
        session_id: String,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[clap(rename_all = "kebab-case")]
pub enum RememberOutputFormatArg {
    Json,
    #[default]
    Md,
}

pub async fn run_cli(cli: Cli) -> Result<String> {
    let source_filter = effective_source(cli.source);
    if let Commands::Note { text } = &cli.command {
        return serialize(&note_response(text.clone())?, cli.pretty);
    }
    if let Commands::Redact(args) = &cli.command {
        return serialize(&redact_response(args, source_filter)?, cli.pretty);
    }
    if let Commands::Sync(args) = &cli.command {
        return serialize(&sync_response(args, source_filter)?, cli.pretty);
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

    let service = QueryService::load()?;

    let response = match cli.command {
        Commands::Projects {
            limit,
            offset,
            sort_by,
            order,
        } => serialize(
            &service.projects(
                source_filter,
                Some(limit),
                offset,
                SortOptions::new(sort_by, order),
            ),
            cli.pretty,
        )?,
        Commands::Sessions {
            project,
            all,
            limit,
            offset,
            sort_by,
            order,
        } => serialize(
            &service.sessions(
                effective_project_scope(project, all).as_deref(),
                source_filter,
                Some(limit),
                offset,
                SortOptions::new(sort_by, order),
            ),
            cli.pretty,
        )?,
        Commands::Messages {
            session,
            project,
            all,
            latest,
            from_message_index,
            to_message_index,
            limit,
            offset,
            sort_by,
            order,
        } => {
            let message_index_range =
                validate_message_index_range(from_message_index, to_message_index)?;
            // When a session ID is provided without an explicit project,
            // skip cwd auto-discovery and search all projects instead.
            let project_scope = if session.is_some() && project.is_none() {
                if source_filter.is_none() {
                    eprintln!(
                        "hint: searching all sources for session; pass --source to narrow the search"
                    );
                }
                None
            } else {
                effective_project_scope(project.clone(), all)
            };

            let mut response = if let Some(latest) = latest {
                service.latest_session_messages(
                    session.as_deref(),
                    project_scope.as_deref(),
                    source_filter,
                    latest.get(),
                    message_index_range,
                )
            } else {
                service.messages(
                    session.as_deref(),
                    project_scope.as_deref(),
                    source_filter,
                    MessageQueryOptions::new(Some(limit), offset, SortOptions::new(sort_by, order))
                        .with_message_index_range(message_index_range),
                )
            };
            if latest.is_none() && response.next_page {
                response.next_command = Some(build_next_messages_command(
                    cli.source,
                    cli.pretty,
                    session.as_deref(),
                    project.as_deref(),
                    all,
                    message_index_range,
                    limit,
                    response.next_offset as usize,
                    sort_by,
                    order,
                ));
            }
            serialize(&response, cli.pretty)?
        }
        Commands::Export { project } => {
            let sort = SortOptions::new(SortBy::Timestamp, SortOrder::Asc);
            if let Some(proj) = project {
                let response = service.messages(
                    None,
                    Some(proj.as_str()),
                    source_filter,
                    MessageQueryOptions::new(None, 0, sort),
                );
                serialize(&response, cli.pretty)?
            } else {
                let (codex_path, claude_name) =
                    resolve_project_from_cwd().context("could not get current directory")?;
                let cursor_name = claude_name.clone();
                let mut messages: Vec<ApiMessage> = Vec::new();
                if source_filter.is_none() || source_filter == Some(SourceFilter::Codex) {
                    let codex = service.messages(
                        None,
                        Some(&codex_path),
                        Some(SourceFilter::Codex),
                        MessageQueryOptions::new(None, 0, sort),
                    );
                    messages.extend(codex.messages);
                }
                if source_filter.is_none() || source_filter == Some(SourceFilter::Claude) {
                    let claude = service.messages(
                        None,
                        Some(&claude_name),
                        Some(SourceFilter::Claude),
                        MessageQueryOptions::new(None, 0, sort),
                    );
                    messages.extend(claude.messages);
                }
                if source_filter.is_none() || source_filter == Some(SourceFilter::Cursor) {
                    let cursor = service.messages(
                        None,
                        Some(&cursor_name),
                        Some(SourceFilter::Cursor),
                        MessageQueryOptions::new(None, 0, sort),
                    );
                    messages.extend(cursor.messages);
                }
                if source_filter.is_none() || source_filter == Some(SourceFilter::Grok) {
                    let grok = service.messages(
                        None,
                        Some(&codex_path),
                        Some(SourceFilter::Grok),
                        MessageQueryOptions::new(None, 0, sort),
                    );
                    messages.extend(grok.messages);
                }
                if source_filter.is_none() || source_filter == Some(SourceFilter::Pi) {
                    let pi = service.messages(
                        None,
                        Some(&codex_path),
                        Some(SourceFilter::Pi),
                        MessageQueryOptions::new(None, 0, sort),
                    );
                    messages.extend(pi.messages);
                }
                messages.sort_by(|a, b| {
                    a.timestamp
                        .cmp(&b.timestamp)
                        .then_with(|| a.session_id.cmp(&b.session_id))
                });
                let total = messages.len() as i64;
                let response = ApiMessagesResponse {
                    messages,
                    total_messages: total,
                    next_page: false,
                    next_offset: total,
                    next_command: None,
                };
                serialize(&response, cli.pretty)?
            }
        }
        Commands::Remember(remember) => {
            let selection = remember.selection();
            let project = match remember.project {
                Some(project) => project,
                None => current_dir_project().context("could not resolve current directory")?,
            };

            let response = ai::remember(
                &service,
                RememberRequest {
                    agent: effective_remember_agent(remember.agent),
                    project: project.as_str(),
                    selection,
                    source: source_filter,
                    instructions: remember.instructions.as_deref(),
                    model: remember.model.as_deref(),
                },
            )
            .await?;
            format_remember_response(&response, remember.output_format, cli.pretty)?
        }
        Commands::Note { text } => serialize(&note_response(text)?, cli.pretty)?,
        Commands::Redact(args) => serialize(&redact_response(&args, source_filter)?, cli.pretty)?,
        Commands::Sync(args) => serialize(&sync_response(&args, source_filter)?, cli.pretty)?,
        Commands::DbInfo {
            project,
            smoke_event,
        } => serialize(&db_info_response(project, smoke_event)?, cli.pretty)?,
    };

    Ok(response)
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
        anyhow::anyhow!("current project is not linked; run `mmr link` before adding notes")
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

fn now_rfc3339() -> Result<String> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .context("format timestamp")
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
) -> Result<SyncDryRunResponse> {
    if !args.dry_run {
        bail!("only `mmr sync --dry-run` is implemented before NHL-277 full sync");
    }

    let store = Store::open_default()?;
    let project = linked_project(&store, args.project.as_deref())?;
    let events = store.events_for_project(&project.id, source_filter_name(source_filter), None)?;

    let mut sync_events = Vec::new();
    let mut syncable_events = 0;
    let mut blocked_events = 0;
    let mut pii_coverage = None;
    for event in events {
        let outcome = scan_text(&event.content_text);
        pii_coverage = Some(outcome.pii_coverage.clone());
        let would_sync = dry_run_allows_sync(&outcome);
        if would_sync {
            syncable_events += 1;
        } else {
            blocked_events += 1;
        }
        let blocked_reasons = dry_run_blocked_reasons(&outcome);
        let status = if outcome.blocks_sync {
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

    Ok(SyncDryRunResponse {
        dry_run: true,
        project_id: project.id,
        remote: "github:<authenticated-user>/mmr-store".to_string(),
        policy_id: DEFAULT_REDACTION_POLICY_ID.to_string(),
        total_events: sync_events.len(),
        syncable_events,
        blocked_events,
        pii_coverage: pii_coverage.unwrap_or_else(|| scan_text("").pii_coverage),
        events: sync_events,
    })
}

fn linked_project(store: &Store, project: Option<&Path>) -> Result<ProjectRecord> {
    let path = match project {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("current_dir")?,
    };
    store.project_by_path(&path)?.ok_or_else(|| {
        anyhow::anyhow!(
            "project is not linked; run `mmr link` before redaction or pass a linked --project"
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

fn dry_run_allows_sync(outcome: &RedactionOutcome) -> bool {
    !outcome.blocks_sync && outcome.pii_coverage.status == PiiCoverageStatus::Available
}

fn dry_run_blocked_reasons(outcome: &RedactionOutcome) -> Vec<String> {
    let mut reasons = Vec::new();
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
    session: Option<&str>,
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

    parts.push("messages".to_string());

    if let Some(sess) = session {
        parts.push(format!("--session {sess}"));
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
    fn remember_all_selector_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "remember",
            "all",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
        ]);

        let parsed = parsed.expect("remember all should parse successfully");
        let Commands::Remember(remember) = parsed.command else {
            panic!("expected remember command");
        };
        assert_eq!(remember.project.as_deref(), Some("/Users/test/proj"));
        assert_eq!(remember.agent, Some(Agent::Gemini));
        assert_eq!(remember.output_format, RememberOutputFormatArg::Md);
        assert!(matches!(
            remember.selection,
            Some(RememberSelectorCommand::All)
        ));
    }

    #[test]
    fn remember_session_selector_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "remember",
            "session",
            "sess-123",
            "--project",
            "/Users/test/proj",
            "--agent",
            "gemini",
            "-O",
            "md",
        ]);

        let parsed = parsed.expect("remember session <id> should parse successfully");
        let Commands::Remember(remember) = parsed.command else {
            panic!("expected remember command");
        };
        assert_eq!(remember.project.as_deref(), Some("/Users/test/proj"));
        assert_eq!(remember.agent, Some(Agent::Gemini));
        assert_eq!(remember.output_format, RememberOutputFormatArg::Md);
        assert!(matches!(
            remember.selection,
            Some(RememberSelectorCommand::Session { session_id }) if session_id == "sess-123"
        ));
    }

    #[test]
    fn remember_without_agent_flag_leaves_agent_unset_for_runtime_defaulting() {
        let parsed =
            Cli::try_parse_from(["mmr", "remember", "--project", "/Users/test/proj", "all"]);

        let parsed = parsed.expect("remember without --agent should parse successfully");
        let Commands::Remember(remember) = parsed.command else {
            panic!("expected remember command");
        };
        assert_eq!(remember.agent, None);
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
