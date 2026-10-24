//! The `isotrail` command-line tool: turns a recorded hike into a pixel-art
//! diorama render

mod bundle;
mod cache;
mod stats;

use std::fs::File;
use std::io::IsTerminal;
use std::path::Path;

use anyhow::Context;
use clap::{ArgAction, Parser, Subcommand};
use isotrail_track::{GpxReader, Track, TrackReader};
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
    /// Prepare the terrain around a GPX route and write debug images of it
    Bundle(bundle::BundleArgs),
    /// Show or clear the cache of downloaded elevation and map data
    Cache(cache::CacheArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    match cli.command {
        Command::Stats(args) => stats::run(&args),
        Command::Bundle(args) => bundle::run(&args),
        Command::Cache(args) => cache::run(&args),
    }
}

fn read_track(path: &Path) -> anyhow::Result<Track> {
    let mut file =
        File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    GpxReader
        .read(&mut file)
        .and_then(Track::clean)
        .with_context(|| format!("could not read {}", path.display()))
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
