use anyhow::Result;

use crate::cache::{
    maybe_spawn_background_refresh, open_cache_db_for_cli, rebuild_cli_cache,
    run_background_refresh_worker,
};
use crate::cli::args::{Cli, Commands};
use crate::query::QueryService;

pub fn run_cli(cli: Cli) -> Result<()> {
    let Cli {
        pretty,
        source,
        quiet,
        refresh,
        command,
    } = cli;

    let command = command.expect("caller ensures this is Some");

    match command {
        Commands::Ingest => rebuild_cli_cache(quiet),
        Commands::BackgroundRefresh => run_background_refresh_worker(),
        Commands::Serve => unreachable!(),
        other => {
            let conn = open_cache_db_for_cli(quiet, refresh)?;
            let source = source.as_deref();
            let service = QueryService::new(&conn);

            let json_output = match other {
                Commands::Projects { limit, offset } => {
                    let result = service.projects(source, limit, offset)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Sessions {
                    project,
                    limit,
                    offset,
                } => {
                    let result = service.sessions(&project, source, limit, offset)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Messages {
                    session,
                    limit,
                    offset,
                } => {
                    let result = service.messages(&session, limit, offset)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Search {
                    query,
                    project,
                    page,
                    limit,
                } => {
                    let result = service.search(&query, project.as_deref(), source, page, limit)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Stats => {
                    let result = service.stats(source)?;
                    if pretty {
                        serde_json::to_string_pretty(&result)?
                    } else {
                        serde_json::to_string(&result)?
                    }
                }
                Commands::Ingest | Commands::Serve | Commands::BackgroundRefresh => {
                    unreachable!("handled above")
                }
            };

            println!("{}", json_output);
            if !refresh {
                let _ = maybe_spawn_background_refresh();
            }
            Ok(())
        }
    }
}
