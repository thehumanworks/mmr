use std::io::{ErrorKind, Write};

use clap::Parser;
use colored::Colorize;
use mmr::cli::{Cli, CliFailure, run_cli};

/// Print one line to stdout. When the reader has gone (`mmr ... | head`),
/// stop quietly with exit 0, as `rg` and other Unix tools do, instead of
/// panicking with a stack trace.
fn print_stdout(text: &str) {
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = writeln!(stdout, "{text}").and_then(|()| stdout.flush()) {
        if error.kind() == ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!(
            "{} failed writing to stdout: {error}",
            "error:".red().bold()
        );
        std::process::exit(1);
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match run_cli(cli).await {
        Ok(json) => {
            if !json.is_empty() {
                print_stdout(&json);
            }
        }
        Err(error) => match error.downcast::<CliFailure>() {
            Ok(failure) => {
                if !failure.stdout.is_empty() {
                    print_stdout(&failure.stdout);
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
