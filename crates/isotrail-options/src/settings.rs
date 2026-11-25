use anyhow::{Context, bail};
use clap::error::ErrorKind;
use clap::{Arg, ArgAction, Command, FromArgMatches};
use toml::value::Datetime;
use toml::{Table, Value};

use crate::{RenderOptions, render_command, usage};

/// Flags that only make sense for one run
pub const NOT_SETTINGS: [&str; 9] = [
    "output",
    "force",
    "config",
    "debug-dir",
    "verbose",
    "quiet",
    "json",
    "help",
    "version",
];

/// The command's flag a setting stands for, if it is one, by its name or
/// an alias such as route-color
pub fn setting<'a>(command: &'a Command, key: &str) -> Option<&'a Arg> {
    command
        .get_arguments()
        .find(|arg| {
            arg.get_long() == Some(key)
                || arg
                    .get_all_aliases()
                    .is_some_and(|aliases| aliases.contains(&key))
        })
        .filter(|arg| {
            !arg.get_long()
                .is_some_and(|long| NOT_SETTINGS.contains(&long))
        })
}

/// The flag's own name for a setting written with an alias
pub fn long_name(command: &Command, key: &str) -> Option<String> {
    setting(command, key)?.get_long().map(str::to_owned)
}

/// The arguments that can't be given with `arg`, whichever of the two
/// declares the conflict
pub fn conflicting<'a>(command: &'a Command, arg: &Arg) -> Vec<&'a Arg> {
    let mut found = command.get_arg_conflicts_with(arg);
    for other in command.get_arguments() {
        let declared = command
            .get_arg_conflicts_with(other)
            .iter()
            .any(|conflict| conflict.get_id() == arg.get_id());
        if declared && !found.iter().any(|f| f.get_id() == other.get_id()) {
            found.push(other);
        }
    }
    found
}

/// The flag a setting stands for, or none for a switch that is off
pub fn flag(key: &str, value: &Value, arg: &Arg) -> anyhow::Result<Option<String>> {
    let switch = matches!(arg.get_action(), ArgAction::SetTrue);
    let scalar = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Integer(number) => Some(number.to_string()),
        Value::Float(number) => Some(number.to_string()),
        Value::Datetime(datetime) => Some(clock_time(datetime)),
        // For the options that take on or off
        Value::Boolean(on) => Some(if *on { "on" } else { "off" }.to_owned()),
        _ => None,
    };
    Ok(match value {
        Value::Boolean(on) if switch => on.then(|| format!("--{key}")),
        _ if switch => bail!("expected true or false"),
        Value::Array(items) => {
            let items: Option<Vec<String>> = items.iter().map(scalar).collect();
            let items = items.context("expected a list of words or numbers")?;
            Some(format!("--{key}={}", items.join(",")))
        }
        value => Some(format!(
            "--{key}={}",
            scalar(value).context("expected a word, number or list")?
        )),
    })
}

/// A TOML local time such as 07:30:00 as the HH:MM that `--time` takes, and
/// anything else as TOML writes it
pub fn clock_time(datetime: &Datetime) -> String {
    match datetime {
        Datetime {
            date: None,
            time: Some(time),
            offset: None,
        } if time.second.unwrap_or(0) == 0 && time.nanosecond.unwrap_or(0) == 0 => {
            format!("{:02}:{:02}", time.hour, time.minute)
        }
        _ => datetime.to_string(),
    }
}

/// Why `command` refuses `flag` given alone, if it does: our own parser's
/// reason, or the first line of clap's text, which names the flag
pub fn reason_alone(command: &Command, flag: &str) -> Option<String> {
    let mut line = vec![command.get_name(), flag];
    // A stand-in for the hike file a command needs
    if command.get_positionals().next().is_some() {
        line.insert(1, "-");
    }
    let error = command.clone().try_get_matches_from(line).err()?;
    if error.kind() == ErrorKind::MissingRequiredArgument {
        return None;
    }
    Some(match std::error::Error::source(&error) {
        Some(reason) if error.kind() == ErrorKind::ValueValidation => reason.to_string(),
        _ => {
            let message = error.to_string();
            let first = message.lines().next().unwrap_or_default();
            first.trim_start_matches("error: ").to_owned()
        }
    })
}

pub fn unknown(key: &str, place: &str) -> String {
    format!("unknown setting {key:?} in {place}")
}

pub fn same_setting(first: &str, key: &str, place: &str) -> String {
    format!("{first} and {key} in {place} are the same setting, so keep one")
}

pub fn not_both(key: &str, other: &str, place: &str) -> String {
    format!("{key} and {other} can't both be set in {place}, so remove one of them")
}

pub fn bad_value(key: &str, place: &str, reason: impl std::fmt::Display) -> String {
    format!("{key} in {place}: {reason}")
}

/// Render settings, checked as a config file's are, in the order `--help`
/// lists them: each the flag's own name and its value
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    entries: Vec<(String, Value)>,
}

impl Settings {
    /// Settings from a JSON object keyed by the long flag names, such as
    /// `{"palette": "aap-64", "aspect": ["story", "wide"]}`; `place` names
    /// where they came from in messages
    pub fn from_json(text: &str, place: &str) -> anyhow::Result<Settings> {
        let json: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| usage(format!("can't read {place}: {error}")))?;
        let serde_json::Value::Object(object) = json else {
            return Err(usage(format!(
                "{place} must hold one object, keyed by option names"
            )));
        };
        let table: Table = object
            .into_iter()
            .map(|(key, value)| (key, from_json(value)))
            .collect();
        Settings::from_table(&table, place)
    }

    /// Settings from a `[render]` table
    pub fn from_table(table: &Table, place: &str) -> anyhow::Result<Settings> {
        let command = render_command();
        let mut found: Vec<(&Arg, &str, &Value)> = Vec::new();
        for (key, value) in table {
            let arg = setting(&command, key).ok_or_else(|| usage(unknown(key, place)))?;
            if let Some((_, first, _)) = found
                .iter()
                .find(|(seen, ..)| seen.get_id() == arg.get_id())
            {
                return Err(usage(same_setting(first, key, place)));
            }
            found.push((arg, key, value));
        }
        for &(arg, key, _) in &found {
            for other in conflicting(&command, arg) {
                if let Some((_, other_key, _)) = found
                    .iter()
                    .find(|(seen, ..)| seen.get_id() == other.get_id())
                {
                    return Err(usage(not_both(key, other_key, place)));
                }
            }
        }
        let mut entries = Vec::new();
        for (arg, key, value) in found {
            let long = arg.get_long().unwrap_or_default();
            let flag =
                flag(long, value, arg).map_err(|error| usage(bad_value(key, place, error)))?;
            let Some(flag) = flag else { continue };
            if let Some(reason) = reason_alone(&command, &flag) {
                return Err(usage(bad_value(key, place, reason)));
            }
            entries.push((long.to_owned(), value.clone()));
        }
        let order = |long: &str| {
            command
                .get_arguments()
                .position(|arg| arg.get_long() == Some(long))
        };
        entries.sort_by_key(|(long, _)| order(long));
        let settings = Settings { entries };
        // Settings fine alone may still clash with each other
        settings
            .options()
            .map_err(|error| usage(format!("{place}: {error:#}")))?;
        Ok(settings)
    }

    /// The settings as `render` takes them
    pub fn options(&self) -> anyhow::Result<RenderOptions> {
        parse(self.flags())
    }

    /// Each setting as a flag, such as `--palette=aap-64`, leaving out
    /// switches that are off
    fn flags(&self) -> Vec<String> {
        let command = render_command();
        self.entries
            .iter()
            .filter_map(|(long, value)| {
                let arg = setting(&command, long)?;
                flag(long, value, arg).ok().flatten()
            })
            .collect()
    }

    /// The settings that differ from the defaults, with their flags
    fn differing(&self) -> Vec<(&str, &Value, String)> {
        let settled = |flags| parse(flags).ok().map(|options| format!("{options:?}"));
        let usual = settled(Vec::new());
        let command = render_command();
        self.entries
            .iter()
            .filter_map(|(long, value)| {
                let flag = flag(long, value, setting(&command, long)?).ok()??;
                (settled(vec![flag.clone()]) != usual).then_some((long.as_str(), value, flag))
            })
            .collect()
    }

    /// An `isotrail render` line for `input` with these settings, leaving
    /// out those at their defaults
    pub fn command_line(&self, input: &str) -> String {
        let mut words = vec!["isotrail".to_owned(), "render".to_owned(), quoted(input)];
        for (_, _, flag) in self.differing() {
            match flag.split_once('=') {
                None => words.push(flag),
                // A value starting with - would read as another option
                Some((name, value)) if value.starts_with('-') && value.parse::<f64>().is_err() => {
                    words.push(format!("{name}={}", quoted(value)));
                }
                Some((name, value)) => {
                    words.push(name.to_owned());
                    words.push(quoted(value));
                }
            }
        }
        words.join(" ")
    }

    /// A config file with these settings in its `[render]` table, leaving
    /// out those at their defaults
    pub fn config_toml(&self) -> String {
        let mut text = String::from("[render]\n");
        for (long, value, _) in self.differing() {
            text.push_str(&format!("{long} = {value}\n"));
        }
        text
    }
}

fn parse(flags: Vec<String>) -> anyhow::Result<RenderOptions> {
    let line = std::iter::once("render".to_owned()).chain(flags);
    let matches = render_command().try_get_matches_from(line)?;
    Ok(RenderOptions::from_arg_matches(&matches)?)
}

/// A JSON value as the TOML value a config file would hold; anything TOML
/// has no word for becomes an empty table, which no option takes
fn from_json(value: serde_json::Value) -> Value {
    use serde_json::Value as Json;
    match value {
        Json::Bool(on) => Value::Boolean(on),
        Json::Number(number) => match number.as_i64() {
            Some(whole) => Value::Integer(whole),
            None => Value::Float(number.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(text) => Value::String(text),
        Json::Array(items) => Value::Array(items.into_iter().map(from_json).collect()),
        Json::Null | Json::Object(_) => Value::Table(Table::new()),
    }
}

/// A word for a shell, in single quotes when it holds anything a shell
/// would read differently
fn quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_.,/:%+=@#".contains(c);
    if !word.is_empty()
        && !word.starts_with('#')
        && !word.starts_with('~')
        && word.chars().all(plain)
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACE: &str = "settings.json";

    fn settled(options: &RenderOptions) -> String {
        format!("{options:?}")
    }

    fn from_json(text: &str) -> Settings {
        Settings::from_json(text, PLACE).unwrap()
    }

    fn json_error(text: &str) -> String {
        format!("{:#}", Settings::from_json(text, PLACE).unwrap_err())
    }

    /// The options a line of flags gives
    fn flagged(flags: &[&str]) -> String {
        let line = std::iter::once("render").chain(flags.iter().copied());
        let matches = render_command().try_get_matches_from(line).unwrap();
        settled(&RenderOptions::from_arg_matches(&matches).unwrap())
    }

    /// The words of a line as a POSIX shell splits it, for the quoting this
    /// module writes
    fn shell_words(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut started = false;
        let mut quoted = false;
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    quoted = !quoted;
                    started = true;
                }
                '\\' if !quoted => {
                    word.extend(chars.next());
                    started = true;
                }
                ' ' if !quoted => {
                    if started {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                }
                c => {
                    word.push(c);
                    started = true;
                }
            }
        }
        if started {
            words.push(word);
        }
        words
    }

    const MANY: &str = r##"{
        "aspect": ["story", "wide"],
        "scale": 5,
        "turn": -30,
        "zoom": 1.5,
        "palette": "aap-64",
        "paths": false,
        "strata": "off",
        "route-color": "#ff0000",
        "sun": [200, -5],
        "title": "Ridge walk",
        "subtitle": "-high",
        "stats": ["distance", "pace"],
        "no-attribution": true,
        "offline": false,
        "loop": 3,
        "hold": "2s",
        "terrain-url": "https://tiles.example/{z}/{x}/{y}.png",
        "trim-start": "1.5km"
    }"##;

    #[test]
    fn json_settles_as_the_same_flags_do() {
        let settings = from_json(MANY);
        let flags = [
            "--aspect=story,wide",
            "--scale=5",
            "--turn=-30",
            "--zoom=1.5",
            "--palette=aap-64",
            "--paths=off",
            "--strata=off",
            "--route-colour=#ff0000",
            "--sun=200,-5",
            "--title=Ridge walk",
            "--subtitle=-high",
            "--stats=distance,pace",
            "--no-attribution",
            "--loop=3",
            "--hold=2s",
            "--terrain-url=https://tiles.example/{z}/{x}/{y}.png",
            "--trim-start=1.5km",
        ];
        assert_eq!(settled(&settings.options().unwrap()), flagged(&flags));
    }

    #[test]
    fn bad_json_is_answered_as_a_config_file_is() {
        for (json, message) in [
            (
                r#"{"shade": 3}"#,
                r#"unknown setting "shade" in settings.json"#,
            ),
            (
                r#"{"output": "a.png"}"#,
                r#"unknown setting "output" in settings.json"#,
            ),
            (
                r#"{"scale": 17}"#,
                "scale in settings.json: expected a whole number from 1 to 16",
            ),
            (
                r#"{"offline": "yes"}"#,
                "offline in settings.json: expected true or false",
            ),
            (
                r#"{"title": null}"#,
                "title in settings.json: expected a word, number or list",
            ),
            (
                r#"{"stats": [{"a": 1}]}"#,
                "stats in settings.json: expected a list of words or numbers",
            ),
            (
                r##"{"route-color": "#ff0000", "route-colour": "#00ff00"}"##,
                "route-color and route-colour in settings.json are the same setting, so keep one",
            ),
            (
                r#"{"size": "800x800", "aspect": "story"}"#,
                "aspect and size can't both be set in settings.json, so remove one of them",
            ),
            (
                r#"["story"]"#,
                "settings.json must hold one object, keyed by option names",
            ),
        ] {
            assert_eq!(json_error(json), message, "{json}");
        }
        assert!(json_error("{").starts_with("can't read settings.json: "));
        let error = Settings::from_json(r#"{"scale": 17}"#, PLACE).unwrap_err();
        assert!(error.is::<crate::UsageError>());
    }

    #[test]
    fn the_command_line_reads_back_as_the_same_settings() {
        let settings = from_json(MANY);
        let line = settings.command_line("My hike's walk.gpx");
        let words = shell_words(&line);
        assert_eq!(words[..3], ["isotrail", "render", "My hike's walk.gpx"]);
        let flags: Vec<&str> = words[3..].iter().map(String::as_str).collect();
        assert_eq!(flagged(&flags), settled(&settings.options().unwrap()));
        for part in [
            r"'My hike'\''s walk.gpx'",
            "--palette aap-64",
            "--turn -30",
            "--route-colour '#ff0000'",
            "--title 'Ridge walk'",
            "--subtitle=-high",
            "--terrain-url 'https://tiles.example/{z}/{x}/{y}.png'",
            "--no-attribution",
        ] {
            assert!(line.contains(part), "{part} in {line}");
        }
    }

    #[test]
    fn the_config_file_reads_back_as_the_same_settings() {
        let settings = from_json(MANY);
        let text = settings.config_toml();
        assert!(text.starts_with("[render]\n"), "{text}");
        let mut file: Table = text.parse().unwrap();
        let Some(Value::Table(table)) = file.remove("render") else {
            panic!("{text}");
        };
        let again = Settings::from_table(&table, "hike.toml").unwrap();
        assert_eq!(
            settled(&again.options().unwrap()),
            settled(&settings.options().unwrap())
        );
        assert!(text.contains("route-colour = \"#ff0000\"\n"), "{text}");
        assert!(text.contains("aspect = [\"story\", \"wide\"]\n"), "{text}");
    }

    #[test]
    fn settings_at_their_defaults_are_left_out() {
        let settings = from_json(
            r#"{"palette": "apollo", "season": "auto", "offline": false, "scale": 3, "aspect": "story"}"#,
        );
        assert_eq!(
            settings.command_line("hike.gpx"),
            "isotrail render hike.gpx"
        );
        assert_eq!(settings.config_toml(), "[render]\n");
        assert_eq!(
            settled(&settings.options().unwrap()),
            settled(&from_json("{}").options().unwrap())
        );
    }

    #[test]
    fn words_a_shell_would_change_are_quoted() {
        for (word, quoted_word) in [
            ("hike.gpx", "hike.gpx"),
            ("1.5km", "1.5km"),
            ("story,wide", "story,wide"),
            ("", "''"),
            ("#ff0000", "'#ff0000'"),
            ("~/hikes", "'~/hikes'"),
            ("a b", "'a b'"),
            ("it's", r"'it'\''s'"),
            ("$HOME", "'$HOME'"),
            ("{z}", "'{z}'"),
        ] {
            assert_eq!(quoted(word), quoted_word, "{word}");
            assert_eq!(shell_words(&quoted(word)), [word], "{word}");
        }
    }
}
