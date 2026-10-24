//! The `isotrail` command-line tool: turns a recorded hike into a pixel-art
//! diorama render

mod cache;
mod stats;

use std::io::IsTerminal;

use clap::{ArgAction, Parser, Subcommand};
use tracing::level_filters::LevelFilter;

/// Turn a recorded hike (GPX) into a pixel-art 3D diorama
#[derive(Debug, Parser)]
#[command(name = "isotrail", bin_name = "isotrail", version)]
struct Cli {
    /// Show more log output (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print distance, elevation and time statistics for a GPX file
    Stats(stats::StatsArgs),
    /// Show or clear the cache of downloaded elevation and map data
    Cache(cache::CacheArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    match cli.command {
        Command::Stats(args) => stats::run(&args),
        Command::Cache(args) => cache::run(&args),
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .without_time()
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
