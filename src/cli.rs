use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::model::{ApiMessage, ApiMessagesResponse, SortBy, SortOptions, SortOrder, SourceFilter};
use crate::query::QueryService;

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
}

pub fn run_cli(cli: Cli) -> Result<String> {
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

fn serialize<T: Serialize>(value: &T, pretty: bool) -> Result<String> {
    if pretty {
        Ok(serde_json::to_string_pretty(value)?)
    } else {
        Ok(serde_json::to_string(value)?)
    }
}
