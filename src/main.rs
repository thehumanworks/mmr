use clap::Parser;
use colored::Colorize;
use mmr::cli::{Cli, CliFailure, run_cli};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match run_cli(cli).await {
        Ok(json) => {
            if !json.is_empty() {
                println!("{json}");
            }
        }
        Err(error) => match error.downcast::<CliFailure>() {
            Ok(failure) => {
                if !failure.stdout.is_empty() {
                    println!("{}", failure.stdout);
                }
                eprintln!("{} {}", "error:".red().bold(), failure.stderr);
                std::process::exit(failure.exit_code);
            }
            Err(error) => {
                eprintln!("{} {}", "error:".red().bold(), error);
                std::process::exit(1);
            }
        },
    }
}
