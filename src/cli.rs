use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::model::{SortBy, SortOptions, SortOrder, SourceFilter};
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
    };

    Ok(response)
}

fn serialize<T: Serialize>(value: &T, pretty: bool) -> Result<String> {
    if pretty {
        Ok(serde_json::to_string_pretty(value)?)
    } else {
        Ok(serde_json::to_string(value)?)
    }
}
