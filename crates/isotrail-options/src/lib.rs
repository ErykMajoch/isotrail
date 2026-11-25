//! The render options of isotrail, defined once: read from flags, config
//! files or JSON, described for a settings page, and written back as a
//! command line or a config file

pub mod animation;
pub mod data;
mod describe;
pub mod encoding;
#[cfg(not(target_arch = "wasm32"))]
mod files;
pub mod frame;
pub mod look;
pub mod overlay;
pub mod scene;
mod settings;
pub mod theme;

use std::collections::HashSet;
use std::fmt;

use chrono::NaiveTime;
use clap::parser::ValueSource;
use clap::{ArgMatches, Args, Command, ValueEnum};
use isotrail_export::DEFAULT_SCALE;
use isotrail_track::Units;

pub use describe::{Kind, OptionInfo, Output, describe};
#[cfg(not(target_arch = "wasm32"))]
pub use files::{file_problem, io_words, plain, read_named, read_text};
pub use settings::{
    NOT_SETTINGS, Settings, bad_value, clock_time, conflicting, flag, long_name, not_both,
    reason_alone, same_setting, setting, unknown,
};

use crate::animation::AnimationArgs;
use crate::data::DataArgs;
use crate::encoding::EncodingArgs;
use crate::frame::{AspectArg, FrameArgs, parse_scale};
use crate::look::LookArgs;
use crate::overlay::OverlayArgs;
use crate::scene::SceneArgs;
use crate::theme::ThemeFileArg;

/// Help headings, in the order `--help` shows them
pub const OUTPUT: &str = "Output";
pub const IMAGE: &str = "Image";
pub const CAMERA: &str = "Camera";
pub const LOOK: &str = "Terrain and look";
pub const SCENE: &str = "Scene, light and route";
pub const TEXT: &str = "Text and overlays";
pub const ANIMATION: &str = "Animation";
pub const PLAYBACK: &str = "Playback and controls";
pub const ENCODING: &str = "Encoding";
pub const DATA: &str = "Data";
pub const GENERAL: &str = "General";

/// A mistake in how isotrail was called that only shows once the command line
/// has been read; reported like clap's own, with exit code 2
#[derive(Debug)]
pub struct UsageError(String);

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

pub fn usage(message: impl Into<String>) -> anyhow::Error {
    UsageError(message.into()).into()
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum UnitsArg {
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

pub fn parse_time(value: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| "expected a local time such as 20:30".to_owned())
}

/// The command and the arguments typed for it on the command line, as
/// opposed to those from config files, the environment or defaults
#[derive(Debug, Default)]
pub struct Typed {
    command: Option<String>,
    ids: HashSet<String>,
}

impl Typed {
    pub fn new(matches: &ArgMatches) -> Typed {
        let Some((command, given)) = matches.subcommand() else {
            return Typed::default();
        };
        let ids = given
            .ids()
            .filter(|id| given.value_source(id.as_str()) == Some(ValueSource::CommandLine))
            .map(|id| id.as_str().to_owned())
            .collect();
        Typed {
            command: Some(command.to_owned()),
            ids,
        }
    }

    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }

    /// By the argument's id, its field name, such as `block_height`
    pub fn has(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}

/// Every setting `render` takes: what it draws and how, apart from the
/// files it reads and writes
#[derive(Debug, Args)]
pub struct RenderOptions {
    /// Image shapes, separated by commas: story, video, portrait, feed, square or wide
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "LIST",
        default_value = "story",
        hide_possible_values = true,
        help_heading = IMAGE
    )]
    pub aspect: Vec<AspectArg>,

    /// Output pixels per drawn pixel, from 1 to 16, dividing both sides of the image
    #[arg(long, value_name = "N", default_value_t = DEFAULT_SCALE, value_parser = parse_scale,
          help_heading = IMAGE)]
    pub scale: u32,

    #[command(flatten)]
    pub frame: FrameArgs,

    #[command(flatten)]
    pub look: LookArgs,

    /// How many times taller than life to draw the terrain, from 0.1 to 100 [default: automatic]
    #[arg(long, value_name = "X", value_parser = data::parse_exaggeration,
          help_heading = LOOK)]
    pub exaggeration: Option<f64>,

    #[command(flatten)]
    pub theme: ThemeFileArg,

    #[command(flatten)]
    pub scene: SceneArgs,

    #[command(flatten)]
    pub overlay: OverlayArgs,

    #[command(flatten)]
    pub animation: AnimationArgs,

    #[command(flatten)]
    pub encoding: EncodingArgs,

    #[command(flatten)]
    pub data: DataArgs,
}

/// The `render` command with its settings and nothing else
pub fn render_command() -> Command {
    RenderOptions::augment_args(Command::new("render").allow_negative_numbers(true))
}
