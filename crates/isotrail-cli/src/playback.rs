use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use isotrail_export::Plays;
use isotrail_render::{
    DEFAULT_ORBIT_DEG, MAX_ZOOM, MAX_ZOOM_RANGE, MIN_DRAW_S, Timeline, parse_within,
};
use serde_json::{Map, Value};

use crate::animation::{PaceArg, parse_duration, parse_orbit, parse_part, parse_rewind};
use crate::encoding::parse_plays;
use crate::usage;

const DEFAULT_DURATION_S: f32 = 10.0;
const DEFAULT_REWIND_S: f32 = 1.0;
/// The controls the embed shows, all of them by default
const CONTROLS: [&str; 4] = ["play", "zoom", "reset", "scrubber"];
/// The words the embed shows or reads out, which `--strings` may replace
const WORDS: [&str; 18] = [
    "play",
    "pause",
    "loading",
    "zoomIn",
    "zoomOut",
    "reset",
    "scrubber",
    "progress",
    "help",
    "wheelHint",
    "poster",
    "diorama",
    "untitled",
    "load",
    "damaged",
    "version",
    "gpu",
    "runtime",
];

/// How the embed plays the hike, and what it shows and says
#[derive(Debug, Args)]
#[command(next_help_heading = crate::PLAYBACK)]
pub struct PlaybackArgs {
    /// Length of one play in seconds, from 1 to 180
    #[arg(long, value_name = "SECONDS", default_value = "10s", value_parser = parse_duration)]
    duration: f32,

    /// Seconds on the scene before the route draws, from 0 to 180 [default: 5% of a play]
    #[arg(long, value_name = "SECONDS", value_parser = parse_part)]
    intro: Option<f32>,

    /// Seconds on the finished hike before the next play, from 0 to 180 [default: 15%]
    #[arg(long, value_name = "SECONDS", value_parser = parse_part)]
    hold: Option<f32>,

    /// Between plays, undraw the route and turn back, for a loop without a jump
    #[arg(long)]
    loop_back: bool,

    /// Seconds a press of play takes to undraw the finished route, from 0 to 10
    #[arg(long, value_name = "SECONDS", default_value = "1s", value_parser = parse_rewind)]
    rewind: f32,

    /// How far the camera turns while the route draws, in degrees from -360 to 360
    #[arg(
        long,
        value_name = "DEG",
        default_value_t = DEFAULT_ORBIT_DEG,
        value_parser = parse_orbit
    )]
    orbit: f32,

    /// Draw the route at an even speed, or following the recorded times
    #[arg(long, value_enum, value_name = "distance|time", default_value_t = PaceArg::Distance,
          hide_possible_values = true)]
    pace: PaceArg,

    /// How many times the embed plays: forever, once, or a number from 1 to 1000
    #[arg(long = "loop", id = "loop", value_name = "forever|once|N", default_value = "once", value_parser = parse_plays)]
    plays: Plays,

    /// Play as soon as the diorama shows, unless the visitor prefers reduced motion
    #[arg(long)]
    autoplay: bool,

    /// Load the diorama when it comes into view, or only when play is pressed
    #[arg(long, value_enum, value_name = "visible|press", default_value_t = LoadArg::Visible,
          hide_possible_values = true)]
    load: LoadArg,

    /// Controls to show: play, zoom, reset and scrubber, or none [default: all]
    #[arg(long, value_name = "LIST|none", value_parser = parse_controls)]
    controls: Option<Controls>,

    /// The largest zoom a visitor can reach, from 1 to 6 [default: 3]
    #[arg(long, value_name = "X", value_parser = parse_max_zoom)]
    max_zoom: Option<f32>,

    /// Language of the buttons and messages, such as pl; en and pl are built in
    #[arg(long, value_name = "CODE", value_parser = parse_lang)]
    lang: Option<String>,

    /// A JSON file of words to use instead of the built-in ones
    #[arg(long, value_name = "FILE")]
    strings: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LoadArg {
    Visible,
    Press,
}

/// The controls to show
#[derive(Debug, Clone, PartialEq, Eq)]
struct Controls(Vec<&'static str>);

fn parse_controls(value: &str) -> Result<Controls, String> {
    if value.trim().eq_ignore_ascii_case("none") {
        return Ok(Controls(Vec::new()));
    }
    let mut shown = Vec::new();
    for name in value
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
    {
        let Some(&known) = CONTROLS.iter().find(|&&known| known == name) else {
            return Err(format!(
                "unknown control {name:?}: expected {} or none",
                CONTROLS.join(", ")
            ));
        };
        if shown.contains(&known) {
            return Err(format!("{known} is listed twice"));
        }
        shown.push(known);
    }
    // In the embed's own order, so the same choice reads the same
    shown.sort_by_key(|name| CONTROLS.iter().position(|known| known == name));
    Ok(Controls(shown))
}

fn parse_max_zoom(value: &str) -> Result<f32, String> {
    parse_within(value, &MAX_ZOOM_RANGE)
}

/// A language tag such as pl or pt-BR
fn parse_lang(value: &str) -> Result<String, String> {
    let value = value.trim();
    let mut parts = value.split('-');
    let language = parts.next().unwrap_or_default();
    let fits = (2..=3).contains(&language.len())
        && language.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|part| {
            (1..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
        });
    if fits {
        Ok(value.to_owned())
    } else {
        Err("expected a language tag such as pl or en-GB".into())
    }
}

impl PlaybackArgs {
    pub fn strings_file(&self) -> Option<&Path> {
        self.strings.as_deref()
    }

    /// Checks that the parts leave time to draw the route, and reads the
    /// words file
    pub fn check(&self) -> anyhow::Result<()> {
        let timeline = Timeline {
            duration_s: self.duration,
            intro_s: self.intro,
            hold_s: self.hold,
            loop_back: self.loop_back,
            ..Timeline::default()
        };
        let draw_s = timeline.draw_s();
        if draw_s < MIN_DRAW_S - 1e-4 {
            return Err(usage(format!(
                "a {}s play leaves {:.2}s to draw the route after the intro{} and the hold; it needs at least {MIN_DRAW_S}s, so shorten --intro or --hold or lengthen --duration",
                self.duration,
                draw_s.max(0.0),
                if self.loop_back { ", the rewind" } else { "" }
            )));
        }
        self.words()?;
        Ok(())
    }

    /// The `--strings` file as one line of JSON, checked against the words
    /// the embed knows
    fn words(&self) -> anyhow::Result<Option<String>> {
        let Some(path) = &self.strings else {
            return Ok(None);
        };
        let named =
            |message: String| usage(format!("the strings file {}: {message}", path.display()));
        let text = crate::read_text(path, "the strings file ").map_err(usage)?;
        let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|error| named(format!("not JSON: {error}")))?;
        let Value::Object(words) = value else {
            return Err(named(
                "expected a JSON object of words, such as {\"play\": \"Start\"}".into(),
            ));
        };
        for (name, text) in &words {
            if !WORDS.contains(&name.as_str()) {
                return Err(named(format!(
                    "unknown word {name:?}; the words are {}",
                    WORDS.join(", ")
                )));
            }
            if !text.is_string() {
                return Err(named(format!("{name} must be text")));
            }
        }
        Ok(Some(Value::Object(Map::from_iter(words)).to_string()))
    }

    /// The choices that differ from the default, as the embed's attribute
    /// names and values; an empty value stands for an attribute on its own
    pub fn attributes(&self) -> anyhow::Result<Vec<(&'static str, String)>> {
        let mut attributes = Vec::new();
        if self.duration != DEFAULT_DURATION_S {
            attributes.push(("duration", self.duration.to_string()));
        }
        if let Some(intro) = self.intro {
            attributes.push(("intro", intro.to_string()));
        }
        if let Some(hold) = self.hold {
            attributes.push(("hold", hold.to_string()));
        }
        if self.loop_back {
            attributes.push(("loop-back", String::new()));
        }
        if self.rewind != DEFAULT_REWIND_S {
            attributes.push(("rewind", self.rewind.to_string()));
        }
        if self.orbit != DEFAULT_ORBIT_DEG {
            attributes.push(("orbit", self.orbit.to_string()));
        }
        if self.pace == PaceArg::Time {
            attributes.push(("pace", "time".to_owned()));
        }
        match self.plays {
            Plays::Forever => attributes.push(("loop", String::new())),
            Plays::Times(n) if n.get() > 1 => attributes.push(("loop", n.to_string())),
            Plays::Times(_) => {}
        }
        if self.autoplay {
            attributes.push(("autoplay", String::new()));
        }
        if self.load == LoadArg::Press {
            attributes.push(("load", "press".to_owned()));
        }
        if let Some(Controls(shown)) = &self.controls
            && shown.len() < CONTROLS.len()
        {
            let list = if shown.is_empty() {
                "none".to_owned()
            } else {
                shown.join(",")
            };
            attributes.push(("controls", list));
        }
        if let Some(zoom) = self.max_zoom.filter(|&zoom| zoom != MAX_ZOOM) {
            attributes.push(("max-zoom", zoom.to_string()));
        }
        if let Some(lang) = &self.lang {
            attributes.push(("lang", lang.clone()));
        }
        if let Some(words) = self.words()? {
            attributes.push(("strings", words));
        }
        Ok(attributes)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    #[command(allow_negative_numbers = true)]
    struct Cli {
        #[command(flatten)]
        playback: PlaybackArgs,
    }

    fn args(line: &[&str]) -> PlaybackArgs {
        Cli::try_parse_from(["isotrail"].iter().chain(line))
            .unwrap()
            .playback
    }

    fn attributes(line: &[&str]) -> Vec<(&'static str, String)> {
        args(line).attributes().unwrap()
    }

    fn owned(pairs: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        pairs
            .iter()
            .map(|&(name, value)| (name, value.to_owned()))
            .collect()
    }

    #[test]
    fn only_choices_that_differ_from_the_default_become_attributes() {
        assert!(attributes(&[]).is_empty());
        let same = [
            "--duration",
            "10s",
            "--orbit",
            "30",
            "--pace",
            "distance",
            "--loop",
            "once",
            "--controls",
            "reset,zoom,scrubber,play",
            "--max-zoom",
            "3",
            "--rewind",
            "1s",
            "--load",
            "visible",
        ];
        assert!(attributes(&same).is_empty());
        let all = attributes(&[
            "--duration",
            "20",
            "--intro",
            "1",
            "--hold",
            "2.5s",
            "--loop-back",
            "--rewind",
            "0",
            "--orbit",
            "-90",
            "--pace",
            "time",
            "--loop",
            "forever",
            "--autoplay",
            "--load",
            "press",
            "--controls",
            "scrubber,play",
            "--max-zoom",
            "6",
            "--lang",
            "pl",
        ]);
        assert_eq!(
            all,
            owned(&[
                ("duration", "20"),
                ("intro", "1"),
                ("hold", "2.5"),
                ("loop-back", ""),
                ("rewind", "0"),
                ("orbit", "-90"),
                ("pace", "time"),
                ("loop", ""),
                ("autoplay", ""),
                ("load", "press"),
                ("controls", "play,scrubber"),
                ("max-zoom", "6"),
                ("lang", "pl"),
            ])
        );
        assert_eq!(attributes(&["--loop", "3"]), owned(&[("loop", "3")]));
        assert_eq!(
            attributes(&["--controls", "none"]),
            owned(&[("controls", "none")])
        );
    }

    #[test]
    fn bad_choices_are_refused() {
        let refused = |line: &[&str]| Cli::try_parse_from(["isotrail"].iter().chain(line)).is_err();
        assert!(refused(&["--controls", "play,map"]));
        assert!(refused(&["--controls", "play,play"]));
        assert!(refused(&["--max-zoom", "7"]));
        assert!(refused(&["--max-zoom", "0.5"]));
        assert!(refused(&["--lang", "polski!"]));
        assert!(refused(&["--duration", "0"]));
        assert!(refused(&["--loop", "0"]));
        assert!(refused(&["--load", "later"]));
        for lang in ["pl", "en-GB", "pt-BR", "zh-Hant-TW"] {
            assert_eq!(parse_lang(lang), Ok(lang.to_owned()));
        }
        let squeezed = args(&["--duration", "2", "--intro", "1", "--hold", "0.8"]).check();
        let message = squeezed.unwrap_err().to_string();
        assert!(message.contains("leaves 0.20s to draw"), "{message}");
        assert!(args(&["--duration", "2", "--intro", "1"]).check().is_ok());
    }

    #[test]
    fn a_strings_file_is_checked_and_written_as_one_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, text: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, text).unwrap();
            path.to_string_lossy().into_owned()
        };
        let good = file(
            "good.json",
            "\u{feff}{\n  \"play\": \"Start\",\n  \"zoomIn\": \"Closer\"\n}\n",
        );
        assert_eq!(
            attributes(&["--strings", &good]),
            owned(&[("strings", r#"{"play":"Start","zoomIn":"Closer"}"#)])
        );
        for (name, text, expected) in [
            (
                "unknown.json",
                r#"{"play": "Start", "map": "Map"}"#,
                "unknown word \"map\"",
            ),
            ("number.json", r#"{"play": 1}"#, "play must be text"),
            ("list.json", r#"["Start"]"#, "expected a JSON object"),
            ("broken.json", r#"{"play": "#, "not JSON"),
        ] {
            let message = args(&["--strings", &file(name, text)])
                .check()
                .unwrap_err()
                .to_string();
            assert!(
                message.contains(expected) && message.contains(name),
                "{message}"
            );
        }
        let missing = dir.path().join("missing.json");
        let missing = missing.to_string_lossy();
        assert!(args(&["--strings", &missing]).check().is_err());
    }

    #[test]
    fn the_words_match_the_embeds() {
        let script = include_str!("../../../web/isotrail-embed.js").replace("\r\n", "\n");
        let english = script
            .split_once("  en: {\n")
            .and_then(|(_, rest)| rest.split_once("\n  },"))
            .expect("the embed's English words")
            .0;
        let names: Vec<&str> = english
            .lines()
            .filter_map(|line| line.strip_prefix("    ")?.split_once(':'))
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, WORDS);
    }
}
