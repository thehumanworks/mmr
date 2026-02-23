use anyhow::Result;
use clap::Parser;
use duckdb::Connection;
use std::sync::{Arc, Mutex};

use crate::api::build_router;
use crate::api::AppState;
use crate::cli::args::{Cli, Commands};
use crate::cli::run_cli;
use crate::db::{create_fts_index, init_db};
use crate::ingest::ingest_all;

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Serve) | None => {}
        Some(_) => {
            return run_cli(cli);
        }
    }

    println!("Initializing DuckDB...");
    let conn = Connection::open_in_memory()?;
    init_db(&conn)?;

    println!("Ingesting conversation history...");
    let stats = ingest_all(&conn)?;
    println!(
        "  Claude: {} messages from {} sessions across {} projects",
        stats.claude_messages, stats.claude_sessions, stats.claude_projects
    );
    println!(
        "  Codex:  {} messages from {} sessions across {} projects",
        stats.codex_messages, stats.codex_sessions, stats.codex_projects
    );
    let total_messages = stats.claude_messages + stats.codex_messages;
    let total_sessions = stats.claude_sessions + stats.codex_sessions;
    println!(
        "  Total:  {} messages, {} sessions",
        total_messages, total_sessions
    );

    println!("Building FTS index...");
    create_fts_index(&conn)?;
    println!("FTS index ready.");

    let state: AppState = Arc::new(Mutex::new(conn));
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3131").await?;
    println!("\nAI Chat History UI available at: http://0.0.0.0:3131");
    println!("Press Ctrl+C to stop.\n");
    axum::serve(listener, app).await?;

    Ok(())
}
