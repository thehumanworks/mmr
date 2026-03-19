use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::Path;

use crate::agent::ai;
use crate::agent::prompt;
use crate::merge::{self, MergeRequest};
use crate::messages::service::QueryService;
use crate::types::{
    Agent, ApiMessage, ApiMessagesResponse, PromptRequest, RememberRequest, RememberResponse,
    RememberSelection, SortBy, SortOptions, SortOrder, SourceFilter, TargetAgent,
};

const ENV_AUTO_DISCOVER_PROJECT: &str = "MMR_AUTO_DISCOVER_PROJECT";
const ENV_DEFAULT_REMEMBER_AGENT: &str = "MMR_DEFAULT_REMEMBER_AGENT";
const ENV_DEFAULT_SOURCE: &str = "MMR_DEFAULT_SOURCE";

#[derive(Parser, Debug)]
#[command(
    name = "mmr",
    about = "Browse AI conversation history from Claude and Codex"
)]
#[command(subcommand_required = true, arg_required_else_help = true)]
pub struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Filter by source: claude, codex (omit to use MMR_DEFAULT_SOURCE or both)
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
    /// All messages for the current project (cwd) or --project, both sources, chronological
    Export {
        /// Project name or path (omit to use current directory)
        #[arg(long)]
        project: Option<String>,
    },
    /// Generate a stateless continuity brief from prior sessions
    Remember(RememberArgs),
    /// Generate an optimized prompt for a target AI coding agent
    Prompt(PromptArgs),
    /// Merge history between sessions or sources
    #[command(after_help = MERGE_AFTER_HELP)]
    Merge(MergeArgs),
}

#[derive(Args, Debug)]
pub struct RememberArgs {
    /// Project name or path (omit to use current directory)
    #[arg(long, short = 'p', global = true)]
    project: Option<String>,
    /// Agent to use (defaults to MMR_DEFAULT_REMEMBER_AGENT or codex)
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

#[derive(Args, Debug)]
pub struct PromptArgs {
    /// What you want to accomplish (the task description)
    query: String,
    /// Target agent the prompt is optimized for
    #[arg(long, value_enum)]
    target: TargetAgent,
    /// Backend agent to use for optimization (defaults to MMR_DEFAULT_REMEMBER_AGENT or codex)
    #[arg(long, value_enum)]
    agent: Option<Agent>,
    /// Project name or path (omit to use current directory)
    #[arg(long, short = 'p')]
    project: Option<String>,
    /// Override backend model
    #[arg(long)]
    model: Option<String>,
}

const MERGE_AFTER_HELP: &str = "\
Examples:
  mmr merge --from-session sess-claude-1 --to-session sess-codex-1
  mmr merge --from-session sess-claude-1 --from-agent claude --to-session sess-codex-1 --to-agent codex
  mmr merge --from-agent codex --to-agent claude --project /Users/test/codex-proj
  mmr merge --from-agent claude --to-agent codex --session sess-claude-1 --project /Users/test/proj";

#[derive(Args, Debug)]
pub struct MergeArgs {
    /// Source session ID for a session-to-session merge
    #[arg(long, conflicts_with = "session")]
    from_session: Option<String>,
    /// Destination session ID for a session-to-session merge
    #[arg(long, conflicts_with = "session")]
    to_session: Option<String>,
    /// Source agent for an agent-to-agent merge or for session disambiguation
    #[arg(long, value_enum)]
    from_agent: Option<SourceFilter>,
    /// Destination agent for an agent-to-agent merge or for session disambiguation
    #[arg(long, value_enum)]
    to_agent: Option<SourceFilter>,
    /// Narrow an agent-to-agent merge to one source session
    #[arg(long, conflicts_with_all = ["from_session", "to_session"])]
    session: Option<String>,
    /// Narrow an agent-to-agent merge to one project
    #[arg(long, conflicts_with_all = ["from_session", "to_session"])]
    project: Option<String>,
}

impl MergeArgs {
    fn into_request(self, global_source: Option<SourceFilter>) -> Result<MergeRequest> {
        if global_source.is_some() {
            anyhow::bail!(
                "merge does not use the global --source flag; use --from-agent/--to-agent or let sessions infer their source"
            );
        }

        match (
            self.from_session,
            self.to_session,
            self.from_agent,
            self.to_agent,
            self.session,
            self.project,
        ) {
            (Some(from_session), Some(to_session), from_agent, to_agent, None, None) => {
                Ok(MergeRequest::SessionToSession {
                    from_session,
                    to_session,
                    from_agent,
                    to_agent,
                })
            }
            (None, None, Some(from_agent), Some(to_agent), session, project) => {
                Ok(MergeRequest::AgentToAgent {
                    from_agent,
                    to_agent,
                    session,
                    project,
                })
            }
            (Some(_), None, _, _, _, _) | (None, Some(_), _, _, _, _) => {
                anyhow::bail!(
                    "session-to-session merges require both --from-session and --to-session"
                )
            }
            _ => anyhow::bail!(
                "choose one merge mode: --from-session/--to-session or --from-agent/--to-agent"
            ),
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
        } => serialize(
            &service.messages(
                session.as_deref(),
                effective_project_scope(project, all).as_deref(),
                source_filter,
                Some(limit),
                offset,
                SortOptions::new(sort_by, order),
            ),
            cli.pretty,
        )?,
        Commands::Export { project } => {
            let sort = SortOptions::new(SortBy::Timestamp, SortOrder::Asc);
            if let Some(proj) = project {
                let response =
                    service.messages(None, Some(proj.as_str()), source_filter, None, 0, sort);
                serialize(&response, cli.pretty)?
            } else {
                let (codex_path, claude_name) =
                    resolve_project_from_cwd().context("could not get current directory")?;
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
                messages.sort_by(|a, b| {
                    a.timestamp
                        .cmp(&b.timestamp)
                        .then_with(|| a.session_id.cmp(&b.session_id))
                });
                let total = messages.len() as i64;
                let response = ApiMessagesResponse {
                    messages,
                    total_messages: total,
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
        Commands::Prompt(prompt_args) => {
            let project = match prompt_args.project {
                Some(project) => project,
                None => current_dir_project().context("could not resolve current directory")?,
            };

            let response = prompt::optimize_prompt(
                &service,
                PromptRequest {
                    agent: effective_remember_agent(prompt_args.agent),
                    target: prompt_args.target,
                    query: &prompt_args.query,
                    project: &project,
                    source: source_filter,
                    model: prompt_args.model.as_deref(),
                },
            )
            .await?;

            prompt::try_copy_to_clipboard(&response.prompt);
            response.prompt
        }
        Commands::Merge(merge_args) => {
            let request = merge_args.into_request(cli.source)?;
            let response = merge::merge(&service, request)?;
            serialize(&response, cli.pretty)?
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
        _ => None,
    }
}

fn effective_remember_agent(cli_agent: Option<Agent>) -> Agent {
    cli_agent
        .or_else(default_remember_agent_from_env)
        .unwrap_or(Agent::Codex)
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
        assert_eq!(parse_source_filter_env(""), None);
        assert_eq!(parse_source_filter_env("invalid"), None);
    }

    #[test]
    fn parse_agent_env_accepts_supported_values() {
        assert_eq!(parse_agent_env("codex"), Some(Agent::Codex));
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
    fn merge_session_mode_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "merge",
            "--from-session",
            "sess-a",
            "--from-agent",
            "claude",
            "--to-session",
            "sess-b",
            "--to-agent",
            "codex",
        ])
        .expect("merge session mode should parse");

        let Commands::Merge(merge) = parsed.command else {
            panic!("expected merge command");
        };
        assert_eq!(merge.from_session.as_deref(), Some("sess-a"));
        assert_eq!(merge.to_session.as_deref(), Some("sess-b"));
        assert_eq!(merge.from_agent, Some(SourceFilter::Claude));
        assert_eq!(merge.to_agent, Some(SourceFilter::Codex));
    }

    #[test]
    fn merge_agent_mode_parses() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "merge",
            "--from-agent",
            "codex",
            "--to-agent",
            "claude",
            "--project",
            "/Users/test/proj",
            "--session",
            "sess-a",
        ])
        .expect("merge agent mode should parse");

        let Commands::Merge(merge) = parsed.command else {
            panic!("expected merge command");
        };
        assert_eq!(merge.from_agent, Some(SourceFilter::Codex));
        assert_eq!(merge.to_agent, Some(SourceFilter::Claude));
        assert_eq!(merge.project.as_deref(), Some("/Users/test/proj"));
        assert_eq!(merge.session.as_deref(), Some("sess-a"));
    }

    #[test]
    fn prompt_command_parses_with_all_flags() {
        let parsed = Cli::try_parse_from([
            "mmr",
            "prompt",
            "add user authentication",
            "--target",
            "claude",
            "--agent",
            "gemini",
            "--project",
            "/Users/test/proj",
            "--model",
            "gemini-2.5-pro",
        ]);

        let parsed = parsed.expect("prompt with all flags should parse");
        let Commands::Prompt(prompt) = parsed.command else {
            panic!("expected prompt command");
        };
        assert_eq!(prompt.query, "add user authentication");
        assert_eq!(prompt.target, TargetAgent::Claude);
        assert_eq!(prompt.agent, Some(Agent::Gemini));
        assert_eq!(prompt.project.as_deref(), Some("/Users/test/proj"));
        assert_eq!(prompt.model.as_deref(), Some("gemini-2.5-pro"));
    }

    #[test]
    fn prompt_command_parses_minimal() {
        let parsed = Cli::try_parse_from(["mmr", "prompt", "fix bug", "--target", "codex"]);

        let parsed = parsed.expect("prompt with minimal args should parse");
        let Commands::Prompt(prompt) = parsed.command else {
            panic!("expected prompt command");
        };
        assert_eq!(prompt.query, "fix bug");
        assert_eq!(prompt.target, TargetAgent::Codex);
        assert_eq!(prompt.agent, None);
        assert_eq!(prompt.project, None);
        assert_eq!(prompt.model, None);
    }

    #[test]
    fn prompt_command_requires_target() {
        assert!(Cli::try_parse_from(["mmr", "prompt", "some query"]).is_err());
    }

    #[test]
    fn prompt_command_requires_query() {
        assert!(Cli::try_parse_from(["mmr", "prompt", "--target", "claude"]).is_err());
    }

    #[test]
    fn prompt_command_rejects_invalid_target() {
        assert!(Cli::try_parse_from(["mmr", "prompt", "some query", "--target", "gpt"]).is_err());
    }

    #[test]
    fn merge_requires_one_mode() {
        let args = MergeArgs {
            from_session: None,
            to_session: None,
            from_agent: Some(SourceFilter::Codex),
            to_agent: None,
            session: None,
            project: None,
        };

        let error = args
            .into_request(None)
            .expect_err("merge should reject incomplete mode");
        assert!(error.to_string().contains("choose one merge mode"));
    }
}
