//! The `isotrail` command-line tool: turns a recorded hike into a pixel-art
//! diorama render

mod bundle;
mod cache;
mod render;
mod stats;
mod style_lab;

use std::fs;
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
    /// Print distance, elevation and time statistics for a GPX file or scene
    /// bundle
    Stats(stats::StatsArgs),
    /// Prepare the terrain around a GPX route and save it with the route as
    /// a scene bundle (.itb)
    Bundle(bundle::BundleArgs),
    /// Render a GPX file or scene bundle as a diorama image
    Render(render::RenderArgs),
    /// Render a hike with every mesher, pixel scale and built-in palette, and
    /// lay the images out on a labelled contact sheet
    StyleLab(style_lab::StyleLabArgs),
    /// Show or clear the cache of downloaded elevation and map data
    Cache(cache::CacheArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    match cli.command {
        Command::Stats(args) => stats::run(&args),
        Command::Bundle(args) => bundle::run(&args),
        Command::Render(args) => render::run(&args),
        Command::StyleLab(args) => style_lab::run(&args),
        Command::Cache(args) => cache::run(&args),
    }
}

fn read_track(path: &Path) -> anyhow::Result<Track> {
    parse_track(&read_file(path)?, path)
}

fn read_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("could not open {}", path.display()))
}

/// A scene bundle by its extension or its first bytes, otherwise GPX
fn is_bundle_file(path: &Path, bytes: &[u8]) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("itb"))
        || isotrail_bundle::is_bundle(bytes)
}

fn parse_track(bytes: &[u8], path: &Path) -> anyhow::Result<Track> {
    GpxReader
        .parse(bytes)
        .and_then(Track::clean)
        .with_context(|| format!("could not read {}", path.display()))
}

/// The track's name, or the file name without its extension
fn title(track: &Track, path: &Path) -> String {
    track
        .name
        .clone()
        .or_else(|| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .unwrap_or_default()
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
