//! The `isotrail` command-line tool: turns a recorded hike into a pixel-art
//! diorama render

mod animation;
mod batch;
mod bundle;
mod cache;
mod config;
mod embed;
mod encoding;
mod frame;
mod look;
mod output;
mod overlay;
mod playback;
mod render;
mod report;
mod scene;
mod season;
mod servers;
mod stats;
mod style_lab;
mod sun;
mod theme;
mod weather;

use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, anyhow};
use clap::error::ErrorKind;
use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueEnum};
use isotrail_track::{GpxReader, Track, TrackReader, Units};
use tracing::level_filters::LevelFilter;

use crate::report::Reason;

/// Help headings, in the order `--help` shows them
const OUTPUT: &str = "Output";
const IMAGE: &str = "Image";
const CAMERA: &str = "Camera";
const LOOK: &str = "Terrain and look";
const SCENE: &str = "Scene, light and route";
const TEXT: &str = "Text and overlays";
const ANIMATION: &str = "Animation";
const PLAYBACK: &str = "Playback and controls";
const ENCODING: &str = "Encoding";
const DATA: &str = "Data";
const GENERAL: &str = "General";

/// Turn a recorded hike (GPX) into a pixel-art 3D diorama
#[derive(Debug, Parser)]
#[command(
    name = "isotrail",
    bin_name = "isotrail",
    version,
    disable_help_flag = true,
    disable_version_flag = true,
    after_help = "Example:\n  isotrail render hike.gpx    Make a picture of the hike, saved as hike.png"
)]
struct Cli {
    /// Show more log output (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = ArgAction::Count, global = true, help_heading = GENERAL)]
    verbose: u8,

    /// Print only errors and the command's own output, such as --json
    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "verbose",
        help_heading = GENERAL
    )]
    quiet: bool,

    /// Print help
    #[arg(short, long, action = ArgAction::Help, global = true, help_heading = GENERAL)]
    help: Option<bool>,

    /// Print the version
    #[arg(short = 'V', long, action = ArgAction::Version, help_heading = GENERAL)]
    version: Option<bool>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Render a GPX file or scene bundle as a still image, GIF or MP4
    #[command(allow_negative_numbers = true)]
    Render(Box<render::RenderArgs>),
    /// Put an interactive diorama of the hike on a web page
    #[command(allow_negative_numbers = true)]
    Embed(Box<embed::EmbedArgs>),
    /// Save the terrain around a GPX route, with the route, as a scene bundle (.itb)
    #[command(allow_negative_numbers = true)]
    Bundle(bundle::BundleArgs),
    /// Make scene files, images and JSON files for a folder of GPX files
    #[command(allow_negative_numbers = true)]
    Batch(Box<batch::BatchArgs>),
    /// Print distance, elevation and time statistics for a hike
    #[command(allow_negative_numbers = true)]
    Stats(stats::StatsArgs),
    /// Compare meshers, pixel scales and palettes on one contact sheet
    #[command(allow_negative_numbers = true)]
    StyleLab(style_lab::StyleLabArgs),
    /// Show or clear the cache of downloaded elevation, map and weather data
    #[command(allow_negative_numbers = true)]
    Cache(cache::CacheArgs),
    /// Write the built-in theme file, to start a theme of your own from
    Theme(theme::ThemeArgs),
}

fn main() -> ExitCode {
    let (cli, typed) = match config::parse() {
        Ok(parsed) => parsed,
        Err((error, typed)) => return report(&error, typed.command()),
    };
    init_logging(cli.verbose, cli.quiet);
    report::set_quiet(cli.quiet);
    output::catch_interrupts();
    let result = match cli.command {
        Command::Stats(args) => stats::run(&args),
        Command::Bundle(args) => bundle::run(&args),
        Command::Batch(args) => batch::run(&args, &typed),
        Command::Render(args) => render::run(&args, &typed),
        Command::Embed(args) => embed::run(&args, &typed),
        Command::StyleLab(args) => style_lab::run(&args, &typed),
        Command::Cache(args) => cache::run(&args),
        Command::Theme(args) => theme::run(&args),
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

/// A hike file that is missing, unreadable or not a hike
#[derive(Debug)]
struct BadInput(String);

impl fmt::Display for BadInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadInput {}

fn report(error: &anyhow::Error, command: Option<&str>) -> ExitCode {
    let reason = reason(error);
    if reason == Reason::Interrupted {
        report::failed(reason, output::STOPPED);
        eprintln!("{}", output::STOPPED);
        return ExitCode::from(output::INTERRUPTED_EXIT);
    }
    if reason == Reason::Usage {
        let mut cli = Cli::command();
        cli.build();
        let message = format!("{error:#}");
        report::failed(reason, &message);
        let clap_error = match command.and_then(|name| cli.find_subcommand_mut(name)) {
            Some(subcommand) => subcommand.error(ErrorKind::ValueValidation, message),
            None => cli.error(ErrorKind::ValueValidation, message),
        };
        clap_error.exit();
    }
    if error.is::<batch::HikesFailed>() {
        print_error(&error.to_string());
        return ExitCode::FAILURE;
    }
    let message = describe(error);
    report::failed(reason, &message);
    print_error(&message);
    ExitCode::FAILURE
}

/// `error: ` and the message on stderr, as clap prints its own
fn print_error(message: &str) {
    let _ = clap::Error::raw(ErrorKind::Io, format!("{message}\n")).print();
}

/// Exits as clap does, after reporting a mistake in the arguments in JSON
/// when asked to
fn exit_with(error: clap::Error) -> ! {
    if error.exit_code() != 0 {
        report::failed(Reason::Usage, &clap_message(&error));
    }
    error.exit()
}

/// Clap's error without its `error:` label, usage line or advice, on one line
fn clap_message(error: &clap::Error) -> String {
    let text = error.render().to_string();
    let text = text.trim_start().trim_start_matches("error:");
    let first = text.split("\n\n").next().unwrap_or_default();
    first
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Which kind of failure `error` is, judged by the first cause that says
fn reason(error: &anyhow::Error) -> Reason {
    if output::interrupted() {
        return Reason::Interrupted;
    }
    error
        .chain()
        .find_map(cause_reason)
        .unwrap_or(Reason::Other)
}

fn cause_reason(cause: &(dyn std::error::Error + 'static)) -> Option<Reason> {
    use isotrail_bundle::BundleError;
    use isotrail_export::ExportError;
    use isotrail_geo::{DemError, FetchError, GeoError, OsmError};

    if cause.is::<output::Interrupted>() {
        return Some(Reason::Interrupted);
    }
    if cause.is::<UsageError>() {
        return Some(Reason::Usage);
    }
    if cause.is::<bundle::NotCached>() {
        return Some(Reason::Offline);
    }
    if cause.is::<BadInput>() || cause.is::<isotrail_track::TrackError>() {
        return Some(Reason::Input);
    }
    if cause.is::<isotrail_render::RenderError>() {
        return Some(Reason::Gpu);
    }
    if let Some(error) = cause.downcast_ref::<BundleError>() {
        return Some(match error {
            BundleError::OlderVersion { .. } | BundleError::NewerVersion { .. } => Reason::Version,
            _ => Reason::Input,
        });
    }
    if let Some(error) = cause.downcast_ref::<ExportError>() {
        return export_reason(error);
    }
    if let Some(error) = cause.downcast_ref::<GeoError>() {
        return Some(match error {
            GeoError::EmptyTrack | GeoError::Track(_) => Reason::Input,
            _ => Reason::Other,
        });
    }
    if let Some(error) = cause.downcast_ref::<FetchError>() {
        return match error {
            FetchError::Offline { .. } => Some(Reason::Offline),
            // Its cause says more
            FetchError::Cache { .. } => None,
            _ => Some(Reason::Network),
        };
    }
    if let Some(error) = cause.downcast_ref::<OsmError>() {
        return Some(if error.is_offline() {
            Reason::Offline
        } else {
            Reason::Network
        });
    }
    if let Some(error) = cause.downcast_ref::<DemError>() {
        return match error {
            DemError::Fetch { source, .. } => cause_reason(source),
            _ => Some(Reason::Network),
        };
    }
    None
}

/// Transparent variants hide their inner error from the chain
fn export_reason(error: &isotrail_export::ExportError) -> Option<Reason> {
    use isotrail_export::ExportError;
    match error {
        ExportError::FfmpegMissing
        | ExportError::FfmpegAt { .. }
        | ExportError::Ffmpeg { .. }
        | ExportError::FfmpegStalled { .. } => Some(Reason::Ffmpeg),
        ExportError::Render(_) => Some(Reason::Gpu),
        ExportError::Write { source, .. } => export_reason(source),
        _ => None,
    }
}

/// The error and its causes on one line, with file errors in words and no
/// cause repeated
fn describe(error: &anyhow::Error) -> String {
    let mut parts: Vec<String> = Vec::new();
    for cause in error.chain() {
        let text = match cause.downcast_ref::<io::Error>() {
            Some(io) => io_words(io),
            None => plain(&cause.to_string()),
        };
        if !text.is_empty() && !parts.iter().any(|part| part.contains(&text)) {
            parts.push(text);
        }
    }
    parts.join(": ")
}

/// What an I/O error means, in words
fn io_words(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "the file doesn't exist".into(),
        io::ErrorKind::PermissionDenied => "permission denied".into(),
        io::ErrorKind::IsADirectory => "it is a folder".into(),
        io::ErrorKind::NotADirectory => "part of the path is a file, not a folder".into(),
        io::ErrorKind::StorageFull => "the disk is full".into(),
        io::ErrorKind::ReadOnlyFilesystem => "the disk is read-only".into(),
        io::ErrorKind::InvalidFilename => "the name can't be used for a file here".into(),
        _ => plain(&error.to_string()),
    }
}

/// A message without the operating system's error number, its full stop or
/// a capital letter at the start of a sentence
fn plain(text: &str) -> String {
    let text = match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => &text[..at],
        _ => text,
    };
    let text = text.trim().trim_end_matches('.');
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if first.is_uppercase() && second.is_lowercase() => first
            .to_lowercase()
            .chain(text[first.len_utf8()..].chars())
            .collect(),
        _ => text.to_owned(),
    }
}

/// Why a file couldn't be read or written, in words
fn file_problem(path: &Path, error: &io::Error) -> String {
    if path.is_dir() {
        "it is a folder".into()
    } else {
        io_words(error)
    }
}

/// Checks the hike file given to a command before anything else: it must be
/// a file, and not a picture or a video
fn check_input(path: &Path) -> anyhow::Result<()> {
    let shown = path.display();
    let bad = |message: String| Err(BadInput(message).into());
    match fs::metadata(path) {
        // Reading it says why
        Err(_) => return Ok(()),
        Ok(meta) if meta.is_dir() => {
            return bad(format!(
                "{shown} is a folder; give a GPX file or a scene bundle (.itb)"
            ));
        }
        Ok(_) => {}
    }
    if let Some(kind) = media(path) {
        return bad(format!(
            "{shown} is {kind}, not a GPX file or a scene bundle (.itb)"
        ));
    }
    Ok(())
}

/// A picture or a video by its first bytes, whatever its name: a bundle
/// may be called hike.png
fn media(path: &Path) -> Option<&'static str> {
    let mut start = [0u8; 12];
    let read = fs::File::open(path)
        .and_then(|mut file| file.read(&mut start))
        .ok()?;
    let start = &start[..read];
    let image = start.starts_with(&[0x89, b'P', b'N', b'G'])
        || start.starts_with(&[0xff, 0xd8, 0xff])
        || start.starts_with(b"GIF8")
        || (start.starts_with(b"RIFF") && start.get(8..12) == Some(b"WEBP"));
    if image {
        return Some("an image");
    }
    // MP4, MOV and HEIC files open with an ftyp box naming their brand
    if start.get(4..8) == Some(b"ftyp") {
        let still = matches!(
            start.get(8..12),
            Some(b"heic" | b"heix" | b"mif1" | b"avif")
        );
        return Some(if still { "an image" } else { "a video" });
    }
    // WebM and Matroska
    start
        .starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        .then_some("a video")
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

/// The hike file's bytes
fn read_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    read_named(path, "").map_err(|error| BadInput(error.to_string()).into())
}

/// The file's bytes, or one message naming `what` it is and the file, such
/// as `can't open the theme dusk.toml: the file doesn't exist`
fn read_named(path: &Path, what: &str) -> anyhow::Result<Vec<u8>> {
    fs::read(path).map_err(|error| {
        anyhow!(
            "can't open {what}{}: {}",
            path.display(),
            file_problem(path, &error)
        )
    })
}

/// A settings file's text, or why it can't be read, naming `what` it is
fn read_text(path: &Path, what: &str) -> Result<String, String> {
    let bytes = read_named(path, what).map_err(|error| error.to_string())?;
    let shown = path.display();
    if bytes.starts_with(&[0xff, 0xfe])
        || bytes.starts_with(&[0xfe, 0xff])
        || utf16_unmarked(&bytes)
    {
        return Err(format!(
            "{what}{shown} is saved as UTF-16 (as PowerShell's > writes it); save it as UTF-8"
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{what}{shown} isn't UTF-8 text"))
}

/// UTF-16 without a byte-order mark: mostly English text puts a zero in
/// every other byte, and UTF-8 text has none
fn utf16_unmarked(bytes: &[u8]) -> bool {
    let start = &bytes[..bytes.len().min(256) & !1];
    let zeros = |parity: usize| {
        start
            .iter()
            .skip(parity)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count()
    };
    let pairs = start.len() / 2;
    let (even, odd) = (zeros(0), zeros(1));
    pairs > 0 && (even.min(odd) == 0) && even.max(odd) * 2 > pairs
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
        .with_context(|| format!("can't read {}", path.display()))
}

/// The track's name, or the file name without its extension
fn title(track: &Track, path: &Path) -> String {
    track
        .name
        .clone()
        .or_else(|| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

fn init_logging(verbose: u8, quiet: bool) {
    let level = match verbose {
        _ if quiet => LevelFilter::ERROR,
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

    #[test]
    fn failures_are_told_apart_through_their_causes() {
        use isotrail_bundle::BundleError;
        use isotrail_export::ExportError;
        use isotrail_geo::{FetchError, GeoError};
        use isotrail_render::RenderError;

        let gpu = || RenderError::UnknownAdapterMode("abacus".into());
        let cases: Vec<(anyhow::Error, Reason)> = vec![
            (output::Interrupted.into(), Reason::Interrupted),
            (usage("--scale 7 doesn't divide 1080"), Reason::Usage),
            (
                BadInput("hike.png is an image".into()).into(),
                Reason::Input,
            ),
            (
                anyhow::Error::new(GeoError::EmptyTrack).context("can't frame the hike"),
                Reason::Input,
            ),
            (
                anyhow::Error::new(BundleError::NewerVersion {
                    found: 7,
                    supported: 6,
                })
                .context("can't read hike.itb"),
                Reason::Version,
            ),
            (ExportError::FfmpegMissing.into(), Reason::Ffmpeg),
            (
                ExportError::Write {
                    path: "hike.mp4".into(),
                    source: Box::new(ExportError::FfmpegStalled {
                        seconds: 60,
                        stderr: String::new(),
                    }),
                }
                .into(),
                Reason::Ffmpeg,
            ),
            (gpu().into(), Reason::Gpu),
            (ExportError::Render(gpu()).into(), Reason::Gpu),
            (
                FetchError::status("https://example.org", 502).into(),
                Reason::Network,
            ),
            (
                FetchError::Offline {
                    url: "https://example.org".into(),
                }
                .into(),
                Reason::Offline,
            ),
            (
                anyhow::Error::new(io::Error::other("disk full")).context("can't write hike.png"),
                Reason::Other,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(reason(&error), expected, "{error:#}");
        }
    }

    #[test]
    fn clap_messages_lose_their_label_and_advice() {
        let error = Cli::command()
            .try_get_matches_from(["isotrail", "stats", "hike.gpx", "--profile", "3"])
            .unwrap_err();
        assert_eq!(
            clap_message(&error),
            "the following required arguments were not provided: --json"
        );
    }

    const HEADINGS: [&str; 11] = [
        OUTPUT, IMAGE, CAMERA, LOOK, SCENE, TEXT, ANIMATION, PLAYBACK, ENCODING, DATA, GENERAL,
    ];
    /// Value names of options that take a number, whose help names the range
    const NUMERIC: [&str; 12] = [
        "N",
        "X",
        "F",
        "DEG",
        "SECONDS",
        "CELLS",
        "PX",
        "WxH",
        "T,R,B,L",
        "PERCENT[,METRES]",
        "AZIMUTH,ELEVATION",
        "forever|once|N",
    ];

    fn built() -> clap::Command {
        let mut cli = Cli::command();
        cli.build();
        cli
    }

    /// Every command, with its name path, such as `cache clear`
    fn commands(command: &clap::Command, path: String, all: &mut Vec<(String, clap::Command)>) {
        all.push((path.clone(), command.clone()));
        for sub in command.get_subcommands() {
            commands(sub, format!("{path} {}", sub.get_name()), all);
        }
    }

    fn every_command() -> Vec<(String, clap::Command)> {
        let mut all = Vec::new();
        commands(&built(), "isotrail".into(), &mut all);
        all
    }

    fn options(command: &clap::Command) -> impl Iterator<Item = &clap::Arg> {
        command.get_arguments().filter(|arg| !arg.is_positional())
    }

    /// The help line without a default written into it
    fn words(help: &str) -> usize {
        let text = match help.find(" [default: ") {
            Some(at) => &help[..at],
            None => help,
        };
        text.split_whitespace().count()
    }

    fn help(arg: &clap::Arg) -> String {
        arg.get_help().map(ToString::to_string).unwrap_or_default()
    }

    #[test]
    fn options_sit_under_the_usage_guides_headings() {
        let cli = built();
        for name in ["render", "embed", "bundle", "batch", "style-lab"] {
            let command = cli.find_subcommand(name).unwrap();
            for arg in options(command) {
                let heading = arg.get_help_heading();
                assert!(
                    heading.is_some_and(|heading| HEADINGS.contains(&heading)),
                    "{name} --{}: {heading:?}",
                    arg.get_id()
                );
            }
        }
        let names: Vec<&str> = cli
            .get_subcommands()
            .map(clap::Command::get_name)
            .filter(|&name| name != "help")
            .collect();
        assert_eq!(
            names,
            [
                "render",
                "embed",
                "bundle",
                "batch",
                "stats",
                "style-lab",
                "cache",
                "theme"
            ]
        );
    }

    #[test]
    fn help_lines_are_short() {
        let mut long = Vec::new();
        for (path, command) in every_command() {
            let about = command.get_about().map(ToString::to_string);
            if let Some(about) = about.filter(|about| words(about) > 15) {
                long.push(format!("{path}: {about}"));
            }
            for arg in options(&command).chain(command.get_positionals()) {
                let help = help(arg);
                if help.is_empty() || words(&help) > 15 || help.ends_with('.') {
                    long.push(format!("{path} {}: {help:?}", arg.get_id()));
                }
            }
        }
        assert!(
            long.is_empty(),
            "{}",
            long.join(
                "
"
            )
        );
    }

    #[test]
    fn number_options_name_their_range() {
        for (path, command) in every_command() {
            for arg in options(&command) {
                let names = arg.get_value_names().unwrap_or_default();
                let numeric = names.iter().any(|name| NUMERIC.contains(&name.as_str()))
                    || (arg.get_long() == Some("scale"));
                let help = help(arg);
                if numeric {
                    assert!(
                        help.contains("from ") && help.contains(" to "),
                        "{path} --{}: {help}",
                        arg.get_id()
                    );
                }
            }
        }
    }

    #[test]
    fn list_defaults_read_as_typed() {
        for (path, command) in every_command() {
            for arg in options(&command) {
                assert!(
                    arg.get_default_values().len() <= 1,
                    "{path} --{}: a default of several values prints with spaces",
                    arg.get_id()
                );
            }
        }
    }

    /// Whether `text` names `--option`, not just a longer one that starts
    /// with it
    fn mentions(text: &str, option: &str) -> bool {
        text.match_indices(option).any(|(at, _)| {
            !text[at + option.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-')
        })
    }

    #[test]
    fn the_usage_guide_lists_every_option() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/usage.md");
        let guide = fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        let mut known = std::collections::BTreeSet::new();
        let mut missing = Vec::new();
        for (path, command) in every_command() {
            for arg in options(&command) {
                let Some(long) = arg.get_long() else { continue };
                if ["help", "version"].contains(&long) {
                    continue;
                }
                let names =
                    std::iter::once(long).chain(arg.get_visible_aliases().into_iter().flatten());
                for name in names {
                    let option = format!("--{name}");
                    if !mentions(&guide, &option) {
                        missing.push(format!("{path} {option}"));
                    }
                    known.insert(option);
                }
            }
        }
        assert!(missing.is_empty(), "not in the usage guide: {missing:?}");
        for row in guide.lines().filter(|line| line.starts_with("| `-")) {
            let cell = row[2..].split(" | ").next().unwrap_or_default();
            for (at, _) in cell.match_indices("--") {
                let option: String = cell[at..]
                    .chars()
                    .take_while(|&c| c == '-' || c.is_ascii_alphanumeric())
                    .collect();
                assert!(
                    known.contains(&option),
                    "the usage guide names {option}, which no command has"
                );
            }
        }
    }

    #[test]
    fn errors_read_as_one_line_of_plain_words() {
        let missing = io::Error::from(io::ErrorKind::NotFound);
        let error = anyhow::Error::new(missing).context("can't open hike.gpx");
        assert_eq!(
            describe(&error),
            "can't open hike.gpx: the file doesn't exist"
        );
        let os = io::Error::other("Access is denied. (os error 5)");
        let error = anyhow::Error::new(os).context("can't write a.png");
        assert_eq!(describe(&error), "can't write a.png: access is denied");
        let error = anyhow!("can't write a.png").context("can't write a.png");
        assert_eq!(describe(&error), "can't write a.png");
        assert_eq!(
            plain("GPU did not finish rendering"),
            "GPU did not finish rendering"
        );
    }
}
