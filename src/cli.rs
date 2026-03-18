use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::agent::ai;
use crate::messages::service::QueryService;
use crate::types::{
    Agent, ApiMessage, ApiMessagesResponse, RememberMode, RememberRequest, RememberResponse,
    SortBy, SortOptions, SortOrder, SourceFilter,
};

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

    /// Filter by source: claude, codex (omit for both)
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
    /// List sessions (optionally filtered by project and/or source)
    Sessions {
        /// Project name or path
        #[arg(long)]
        project: Option<String>,
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
    /// Get messages (optionally filtered by session, project, and/or source)
    Messages {
        /// Session ID
        #[arg(long)]
        session: Option<String>,
        /// Project name or path
        #[arg(long)]
        project: Option<String>,
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
    /// Generate a continuity brief from prior sessions and continue follow-ups
    Remember {
        /// Project name or path (omit to use current directory)
        #[arg(long)]
        project: Option<String>,
        /// Agent to use
        #[arg(long, value_enum, default_value = "codex")]
        agent: Agent,
        /// Session selection mode
        #[arg(long, value_enum, default_value = "latest")]
        mode: RememberModeArg,
        /// Continue from a previous Gemini interaction ID
        #[arg(long = "continue-from")]
        continue_from: Option<String>,
        /// Follow-up user message for a continuation (requires --continue-from)
        #[arg(long = "follow-up")]
        follow_up: Option<String>,
        /// Override the output format and rules portion of the system instructions (applies to first message and continuations)
        #[arg(long)]
        instructions: Option<String>,
        /// Output format for remember results
        #[arg(
            short = 'O',
            long = "output-format",
            value_enum,
            default_value = "json"
        )]
        output_format: RememberOutputFormatArg,
        /// Gemini model to use
        #[arg(long)]
        model: Option<String>,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "kebab-case")]
pub enum RememberModeArg {
    Latest,
    All,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[clap(rename_all = "kebab-case")]
pub enum RememberOutputFormatArg {
    Json,
    Md,
}

impl From<RememberModeArg> for RememberMode {
    fn from(value: RememberModeArg) -> Self {
        match value {
            RememberModeArg::Latest => RememberMode::Latest,
            RememberModeArg::All => RememberMode::All,
        }
    }
}

pub async fn run_cli(cli: Cli) -> Result<String> {
    let service = QueryService::load()?;

    let response = match cli.command {
        Commands::Projects {
            limit,
            offset,
            sort_by,
            order,
        } => serialize(
            &service.projects(
                cli.source,
                Some(limit),
                offset,
                SortOptions::new(sort_by, order),
            ),
            cli.pretty,
        )?,
        Commands::Sessions {
            project,
            limit,
            offset,
            sort_by,
            order,
        } => serialize(
            &service.sessions(
                project.as_deref(),
                cli.source,
                Some(limit),
                offset,
                SortOptions::new(sort_by, order),
            ),
            cli.pretty,
        )?,
        Commands::Messages {
            session,
            project,
            limit,
            offset,
            sort_by,
            order,
        } => serialize(
            &service.messages(
                session.as_deref(),
                project.as_deref(),
                cli.source,
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
                    service.messages(None, Some(proj.as_str()), cli.source, None, 0, sort);
                serialize(&response, cli.pretty)?
            } else {
                let (codex_path, claude_name) =
                    resolve_project_from_cwd().context("could not get current directory")?;
                let mut messages: Vec<ApiMessage> = Vec::new();
                if cli.source.is_none() || cli.source == Some(SourceFilter::Codex) {
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
                if cli.source.is_none() || cli.source == Some(SourceFilter::Claude) {
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
        Commands::Remember {
            project,
            agent,
            mode,
            continue_from,
            follow_up,
            instructions,
            output_format,
            model,
        } => {
            let project = match project {
                Some(project) => project,
                None => current_dir_project().context("could not resolve current directory")?,
            };
            let response = ai::remember(
                &service,
                RememberRequest {
                    agent,
                    project: project.as_str(),
                    source: cli.source,
                    mode: mode.into(),
                    continue_from: continue_from.as_deref(),
                    follow_up: follow_up.as_deref(),
                    instructions: instructions.as_deref(),
                    model: model.as_deref(),
                },
            )
            .await?;
            format_remember_response(&response, output_format, cli.pretty)?
        }
    };

    Ok(response)
}

/// Resolve current working directory to (codex_project_path, claude_project_name).
/// Codex uses the canonical path as-is; Claude uses path with '/' replaced by '-'
/// (e.g. /Users/mish/proj -> -Users-mish-proj).
fn resolve_project_from_cwd() -> Result<(String, String)> {
    let path = std::env::current_dir()
        .context("current_dir")?
        .canonicalize()
        .context("canonicalize")?;
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
    let brief = if response.text.trim().is_empty() {
        "(No continuity brief returned.)"
    } else {
        response.text.trim()
    };
    let thread_or_interaction_id = if response.thread_or_interaction_id.is_none() {
        "(none)"
    } else {
        response.thread_or_interaction_id.as_ref().unwrap().trim()
    };

    match response.agent {
        Agent::Gemini => format!(
            "{}\n\n---\nInteraction ID: `{}`",
            brief, thread_or_interaction_id
        ),
        Agent::Codex => format!(
            "{}\n\n---\nThread ID: `{}`",
            brief, thread_or_interaction_id
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_markdown_transformation_includes_summary_and_interaction_id() {
        let response = RememberResponse {
            agent: Agent::Gemini,
            text: "# Continuity Brief\n\nSummary body".to_string(),
            thread_or_interaction_id: Some("abc-123".to_string()),
        };

        let markdown = remember_response_to_markdown(&response);
        assert!(markdown.contains("# Continuity Brief"));
        assert!(markdown.contains("Summary body"));
        assert!(markdown.contains("Interaction ID: `abc-123`"));
    }

    #[test]
    fn remember_markdown_transformation_handles_empty_values() {
        let response = RememberResponse {
            agent: Agent::Gemini,
            text: "  ".to_string(),
            thread_or_interaction_id: None,
        };

        let markdown = remember_response_to_markdown(&response);
        assert!(markdown.contains("(No continuity brief returned.)"));
        assert!(markdown.contains("Interaction ID: `(none)`"));
    }

    #[test]
    fn remember_markdown_transformation_trims_outer_whitespace() {
        let response = RememberResponse {
            agent: Agent::Gemini,
            text: "\n  line one\nline two  \n".to_string(),
            thread_or_interaction_id: Some("  id-1  ".to_string()),
        };

        let markdown = remember_response_to_markdown(&response);
        assert!(markdown.contains("line one\nline two"));
        assert!(markdown.contains("Interaction ID: `id-1`"));
    }
}
