use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::Path;

use crate::agent::ai;
use crate::messages::service::QueryService;
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
    about = "Browse AI conversation history from Claude, Codex, and Cursor"
)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Filter by source: claude, codex, cursor (omit to use MMR_DEFAULT_SOURCE or all)
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
    /// Backend model override (currently used by Cursor and Gemini)
    #[arg(long, global = true)]
    model: Option<String>,
    /// Session selector (omit for latest)
    #[command(subcommand)]
    selection: Option<RememberSelectorCommand>,
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
    let service = QueryService::load()?;
    let source_filter = effective_source(cli.source);

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
            limit,
            offset,
            sort_by,
            order,
        } => {
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

            let mut response = service.messages(
                session.as_deref(),
                project_scope.as_deref(),
                source_filter,
                Some(limit),
                offset,
                SortOptions::new(sort_by, order),
            );
            if response.next_page {
                response.next_command = Some(build_next_messages_command(
                    cli.source,
                    cli.pretty,
                    session.as_deref(),
                    project.as_deref(),
                    all,
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
                let response =
                    service.messages(None, Some(proj.as_str()), source_filter, None, 0, sort);
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
                        None,
                        0,
                        sort,
                    );
                    messages.extend(codex.messages);
                }
                if source_filter.is_none() || source_filter == Some(SourceFilter::Claude) {
                    let claude = service.messages(
                        None,
                        Some(&claude_name),
                        Some(SourceFilter::Claude),
                        None,
                        0,
                        sort,
                    );
                    messages.extend(claude.messages);
                }
                if source_filter.is_none() || source_filter == Some(SourceFilter::Cursor) {
                    let cursor = service.messages(
                        None,
                        Some(&cursor_name),
                        Some(SourceFilter::Cursor),
                        None,
                        0,
                        sort,
                    );
                    messages.extend(cursor.messages);
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
    };

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
        _ => None,
    }
}

fn effective_remember_agent(cli_agent: Option<Agent>) -> Agent {
    cli_agent
        .or_else(default_remember_agent_from_env)
        .unwrap_or(Agent::Cursor)
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
}
