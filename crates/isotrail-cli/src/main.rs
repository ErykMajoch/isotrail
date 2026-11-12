//! The `isotrail` command-line tool: turns a recorded hike into a pixel-art
//! diorama render

mod animation;
mod bundle;
mod cache;
mod config;
mod embed;
mod frame;
mod look;
mod output;
mod overlay;
mod render;
mod scene;
mod season;
mod stats;
mod style_lab;
mod sun;
mod weather;

use std::fmt;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Context;
use clap::error::ErrorKind;
use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueEnum};
use isotrail_track::{GpxReader, Track, TrackReader, Units};
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
    Render(Box<render::RenderArgs>),
    /// Write a scene bundle, a poster image and an HTML snippet that put an
    /// interactive diorama of the hike on a web page
    Embed(Box<embed::EmbedArgs>),
    /// Render a hike with every mesher, pixel scale and built-in palette, and
    /// lay the images out on a labelled contact sheet
    StyleLab(style_lab::StyleLabArgs),
    /// Show or clear the cache of downloaded elevation and map data
    Cache(cache::CacheArgs),
}

fn main() -> ExitCode {
    let (cli, typed) = match config::parse() {
        Ok(parsed) => parsed,
        Err((error, typed)) => return report(&error, typed.command()),
    };
    init_logging(cli.verbose);
    output::catch_interrupts();
    let result = match cli.command {
        Command::Stats(args) => stats::run(&args),
        Command::Bundle(args) => bundle::run(&args),
        Command::Render(args) => render::run(&args, &typed),
        Command::Embed(args) => embed::run(&args, &typed),
        Command::StyleLab(args) => style_lab::run(&args, &typed),
        Command::Cache(args) => cache::run(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report(&error, typed.command()),
    }
}

/// A mistake in how isotrail was called that only shows once the command line
/// has been read; reported like clap's own, with exit code 2
#[derive(Debug)]
struct UsageError(String);

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

fn usage(message: impl Into<String>) -> anyhow::Error {
    UsageError(message.into()).into()
}

fn report(error: &anyhow::Error, command: Option<&str>) -> ExitCode {
    if output::interrupted() || error.is::<output::Interrupted>() {
        eprintln!("Stopped; nothing was left half-written");
        return ExitCode::from(output::INTERRUPTED_EXIT);
    }
    if error.is::<UsageError>() {
        let mut cli = Cli::command();
        cli.build();
        let message = format!("{error:#}");
        let clap_error = match command.and_then(|name| cli.find_subcommand_mut(name)) {
            Some(subcommand) => subcommand.error(ErrorKind::ValueValidation, message),
            None => cli.error(ErrorKind::ValueValidation, message),
        };
        clap_error.exit();
    }
    eprintln!("Error: {error:?}");
    ExitCode::FAILURE
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum UnitsArg {
    Metric,
    Imperial,
}

impl From<UnitsArg> for Units {
    fn from(units: UnitsArg) -> Self {
        match units {
            UnitsArg::Metric => Units::Metric,
            UnitsArg::Imperial => Units::Imperial,
        }
    }
}

fn read_track(path: &Path) -> anyhow::Result<Track> {
    let bytes = read_file(path)?;
    if isotrail_bundle::is_bundle(&bytes) {
        return Err(usage(format!(
            "{} is already a scene bundle, not a GPX file; render, embed and stats take it as it is",
            path.display()
        )));
    }
    parse_track(&bytes, path)
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
