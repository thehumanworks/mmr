use clap::Parser;
use colored::Colorize;
use mmr::cli::{Cli, run_cli};

fn main() {
    let cli = Cli::parse();

    match run_cli(cli) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("{} {}", "error:".red().bold(), error);
            std::process::exit(1);
        }
    }
}
