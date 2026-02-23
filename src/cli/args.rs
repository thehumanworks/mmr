use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mmr", about = "Search and browse AI conversation history")]
pub struct Cli {
    /// Pretty-print JSON output
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Filter by source: claude, codex (default: codex)
    #[arg(long, global = true, default_value = "codex")]
    pub source: Option<String>,

    /// Suppress ingestion progress (stderr)
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Refresh cache incrementally before running a query command
    #[arg(short = 'r', long, global = true)]
    pub refresh: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List all projects
    Projects {
        /// Maximum number of projects to return
        #[arg(long)]
        limit: Option<usize>,
        /// Number of projects to skip
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// List sessions for a project
    Sessions {
        /// Project name
        #[arg(long)]
        project: String,
        /// Maximum number of sessions to return
        #[arg(long)]
        limit: Option<usize>,
        /// Number of sessions to skip
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Get messages for a session
    Messages {
        /// Session ID
        #[arg(long)]
        session: String,
        /// Return only the last N messages
        #[arg(long)]
        limit: Option<usize>,
        /// Number of messages to skip
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Search across all conversations
    Search {
        /// Search query
        query: String,
        /// Filter by project name
        #[arg(long)]
        project: Option<String>,
        /// Page number (0-indexed)
        #[arg(long, default_value = "0")]
        page: usize,
        /// Results per page
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Show usage statistics
    Stats,
    /// (Re)ingest conversation history and rebuild the CLI cache
    #[command(alias = "refresh")]
    Ingest,
    /// Start the web server (default when no subcommand given)
    Serve,
    #[command(name = "__background-refresh", hide = true)]
    BackgroundRefresh,
}
