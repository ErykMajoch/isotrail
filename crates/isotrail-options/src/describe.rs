use std::ops::RangeInclusive;

use clap::{Arg, ArgAction};
use isotrail_export::{MAX_CRF, MAX_MP4_FPS};
use isotrail_render::{
    BASE_BLOCKS_RANGE, BLOCK_HEIGHT_RANGE, HEADING_RANGE, OCCLUSION_RANGE, OUTLINES_RANGE,
    PITCH_RANGE, PROP_DENSITY_RANGE, Palette, ROUTE_WIDTH_RANGE, SOFT_SHADOWS_RANGE, TURN_RANGE,
    ZOOM_RANGE,
};
use serde::Serialize;

use crate::animation::MAX_MP4_DURATION_S;
use crate::data::{DETAIL_RANGE, EXAGGERATION_RANGE};
use crate::encoding::MAX_PLAYS;
use crate::frame::{MAX_SAFE_SHARE, SCALE_RANGE, SIDE_RANGE};
use crate::{ANIMATION, NOT_SETTINGS, render_command};

/// One render option, as a settings page needs it
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OptionInfo {
    /// The long flag name, which is also the config key and the JSON key
    pub name: String,
    /// Other names it answers to, such as route-color
    pub aliases: Vec<String>,
    /// The help heading it sits under
    pub group: String,
    /// The help line, without the default
    pub help: String,
    pub kind: Kind,
    /// What `--help` shows for the value, such as HH:MM
    pub value_name: Option<String>,
    /// The words it takes
    pub choices: Vec<String>,
    /// Whether it takes values besides its choices, such as a colour
    pub open: bool,
    /// The lowest and highest number it takes: for a size each side, for a
    /// safe area each margin
    pub range: Option<(f64, f64)>,
    pub default: Option<String>,
    /// The files it changes
    pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// On when given, with no value
    Switch,
    /// One of its choices
    Choice,
    /// Several of its choices, separated by commas
    List,
    Number,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    Png,
    Gif,
    Mp4,
}

/// Value names of the options that take one number
const NUMBERS: [&str; 7] = ["N", "X", "F", "DEG", "SECONDS", "CELLS", "PX"];

/// Every render option, in the order the command defines them
pub fn describe() -> Vec<OptionInfo> {
    let command = render_command();
    command
        .get_arguments()
        .filter(|arg| {
            arg.get_long()
                .is_some_and(|long| !NOT_SETTINGS.contains(&long))
        })
        .map(info)
        .collect()
}

fn info(arg: &Arg) -> OptionInfo {
    let name = arg.get_long().unwrap_or_default().to_owned();
    let full = arg.get_help().map(ToString::to_string).unwrap_or_default();
    let (help, written_default) = match full.split_once(" [default: ") {
        Some((help, default)) => (
            help.to_owned(),
            Some(default.trim_end_matches(']').to_owned()),
        ),
        None => (full.clone(), None),
    };
    let switch = matches!(arg.get_action(), ArgAction::SetTrue);
    let value_name = arg
        .get_value_names()
        .and_then(|names| names.first())
        .map(ToString::to_string)
        .filter(|_| !switch);
    let default = arg
        .get_default_values()
        .first()
        .map(|value| value.to_string_lossy().into_owned())
        .or(written_default)
        .filter(|_| !switch);
    let list = arg.get_value_delimiter().is_some()
        || value_name
            .as_deref()
            .is_some_and(|name| name.starts_with("LIST"));
    let (choices, open) = if switch {
        (Vec::new(), false)
    } else {
        choices(&name, arg, value_name.as_deref(), default.as_deref(), list)
    };
    let kind = if switch {
        Kind::Switch
    } else if list {
        Kind::List
    } else if !choices.is_empty() {
        Kind::Choice
    } else if value_name
        .as_deref()
        .is_some_and(|name| NUMBERS.contains(&name))
    {
        Kind::Number
    } else {
        Kind::Text
    };
    OptionInfo {
        aliases: arg
            .get_all_aliases()
            .unwrap_or_default()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        group: arg.get_help_heading().unwrap_or_default().to_owned(),
        outputs: outputs(&name, arg.get_help_heading()),
        range: range(&name),
        name,
        help,
        kind,
        value_name,
        choices,
        open,
        default,
    }
}

/// The words an option takes, and whether it takes others too. They come
/// from its parser, from a value name such as `auto|on|off`, or for a list
/// its own parser reads, from its default
fn choices(
    name: &str,
    arg: &Arg,
    value_name: Option<&str>,
    default: Option<&str>,
    list: bool,
) -> (Vec<String>, bool) {
    if name == "palette" {
        let names = Palette::builtin_names().chain(["none"]).map(str::to_owned);
        return (names.collect(), true);
    }
    let mut words: Vec<String> = Vec::new();
    let mut add = |word: &str| {
        if !words.iter().any(|known| known == word) {
            words.push(word.to_owned());
        }
    };
    let parsed: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|value| value.get_name().to_owned())
        .collect();
    if list && parsed.is_empty() {
        for word in default
            .unwrap_or_default()
            .split(',')
            .filter(|w| is_word(w))
        {
            add(word);
        }
    }
    for word in &parsed {
        add(word);
    }
    let mut open = false;
    for part in value_name.unwrap_or_default().split('|') {
        if is_word(part) {
            add(part);
        } else if part != "LIST" && value_name.is_some_and(|name| name.contains('|')) {
            open = true;
        }
    }
    // Such as sizes
    if list && words.is_empty() {
        open = true;
    }
    (words, open)
}

fn is_word(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_lowercase() || c == '-')
}

fn outputs(name: &str, group: Option<&str>) -> Vec<Output> {
    match name {
        "gif-scale" => vec![Output::Gif],
        "loop" => vec![Output::Gif, Output::Mp4],
        "crf" | "preset" | "ffmpeg" => vec![Output::Mp4],
        _ if group == Some(ANIMATION) => vec![Output::Gif, Output::Mp4],
        _ => vec![Output::Png, Output::Gif, Output::Mp4],
    }
}

/// The numbers an option takes, as its parser checks them
fn range(name: &str) -> Option<(f64, f64)> {
    let whole = |range: RangeInclusive<u32>| (f64::from(*range.start()), f64::from(*range.end()));
    let float = |range: RangeInclusive<f32>| (f64::from(*range.start()), f64::from(*range.end()));
    Some(match name {
        "scale" | "gif-scale" => whole(SCALE_RANGE),
        "size" => whole(SIDE_RANGE),
        "safe-area" => (0.0, f64::from(MAX_SAFE_SHARE)),
        "pitch" => float(PITCH_RANGE),
        "turn" | "orbit" => float(TURN_RANGE),
        "heading" => float(HEADING_RANGE),
        "zoom" => float(ZOOM_RANGE),
        "block-height" => float(BLOCK_HEIGHT_RANGE),
        "occlusion" => float(OCCLUSION_RANGE),
        "soft-shadows" => float(SOFT_SHADOWS_RANGE),
        "exaggeration" => (*EXAGGERATION_RANGE.start(), *EXAGGERATION_RANGE.end()),
        "prop-density" => float(PROP_DENSITY_RANGE),
        "base-blocks" => whole(BASE_BLOCKS_RANGE),
        "outlines" => float(OUTLINES_RANGE),
        "route-width" => whole(ROUTE_WIDTH_RANGE),
        "duration" => (1.0, f64::from(MAX_MP4_DURATION_S)),
        "intro" | "hold" => (0.0, f64::from(MAX_MP4_DURATION_S)),
        "fps" | "step-fps" => whole(1..=MAX_MP4_FPS),
        "loop" => (1.0, f64::from(MAX_PLAYS)),
        "crf" => (0.0, f64::from(MAX_CRF)),
        "detail" => whole(DETAIL_RANGE),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;
    use crate::reason_alone;

    /// The render section of the usage guide
    fn guide() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/usage.md");
        let text = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        let start = text.find("\n## render\n").unwrap();
        let end = start + text[start..].find("\n## bundle\n").unwrap();
        text[start..end].to_owned()
    }

    #[test]
    fn every_option_in_the_usage_guides_render_tables_and_no_other() {
        let guide = guide();
        let mut listed = BTreeSet::new();
        for row in guide.lines().filter(|line| line.starts_with("| `-")) {
            let cell = row[2..].split(" | ").next().unwrap_or_default();
            for (at, _) in cell.match_indices("--") {
                let name: String = cell[at + 2..]
                    .chars()
                    .take_while(|&c| c == '-' || c.is_ascii_alphanumeric())
                    .collect();
                if !NOT_SETTINGS.contains(&name.as_str()) {
                    listed.insert(name);
                }
            }
        }
        let options = describe();
        let described: BTreeSet<String> = options.iter().map(|o| o.name.clone()).collect();
        assert_eq!(described, listed);
        for alias in options.iter().flat_map(|o| &o.aliases) {
            assert!(guide.contains(&format!("`--{alias}`")), "--{alias}");
        }
    }

    fn number(value: f64) -> String {
        value.to_string()
    }

    #[test]
    fn ranges_are_the_ones_the_help_names_and_the_parser_checks() {
        let command = render_command();
        for option in describe() {
            let name = &option.name;
            if option.kind == Kind::Number {
                assert!(option.range.is_some(), "--{name} has no range");
            }
            let Some((min, max)) = option.range else {
                continue;
            };
            // These two go up to another option's value
            if !["gif-scale", "step-fps"].contains(&name.as_str()) {
                let help = &option.help;
                assert!(
                    help.contains(&format!("from {}", number(min))),
                    "{name}: {help}"
                );
                assert!(help.contains(&number(max)), "{name}: {help}");
            }
            let value = |n: f64| match name.as_str() {
                "size" => format!("{n}x{n}"),
                _ => number(n),
            };
            for fits in [min, max] {
                let flag = format!("--{name}={}", value(fits));
                assert_eq!(reason_alone(&command, &flag), None, "{flag}");
            }
            let step = if min.fract() == 0.0 && max.fract() == 0.0 {
                1.0
            } else {
                0.01
            };
            for past in [min - step, max + step] {
                let flag = format!("--{name}={}", value(past));
                assert!(reason_alone(&command, &flag).is_some(), "{flag}");
            }
        }
    }

    #[test]
    fn options_say_what_they_take() {
        let options = describe();
        let find = |name: &str| options.iter().find(|o| o.name == name).unwrap().clone();
        let offline = find("offline");
        assert_eq!(offline.kind, Kind::Switch);
        assert!(offline.choices.is_empty() && offline.default.is_none());
        let markers = find("markers");
        assert_eq!(markers.kind, Kind::List);
        assert_eq!(markers.choices, ["start", "finish", "head", "none"]);
        let background = find("background");
        assert_eq!((background.kind, background.open), (Kind::Choice, true));
        assert_eq!(background.choices, ["sky", "transparent"]);
        assert_eq!(background.default.as_deref(), Some("sky"));
        let heading = find("heading");
        assert_eq!(heading.kind, Kind::Number);
        assert_eq!(heading.default.as_deref(), Some("along the hike"));
        assert!(!heading.help.contains("[default"), "{}", heading.help);
        assert_eq!(find("route-colour").aliases, ["route-color"]);
        assert!(find("palette").choices.contains(&"none".to_owned()));
        assert_eq!(find("fps").outputs, [Output::Gif, Output::Mp4]);
        assert_eq!(find("crf").outputs, [Output::Mp4]);
        assert_eq!(find("gif-scale").outputs, [Output::Gif]);
        assert_eq!(find("season").outputs.len(), 3);
        let size = find("size");
        assert_eq!((size.kind, size.open), (Kind::List, true));
        // Choices are words the option takes
        let command = render_command();
        for option in &options {
            for choice in &option.choices {
                let flag = format!("--{}={choice}", option.name);
                assert_eq!(reason_alone(&command, &flag), None, "{flag}");
            }
        }
    }
}
