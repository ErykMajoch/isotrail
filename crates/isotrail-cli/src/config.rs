use std::collections::{BTreeMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command, CommandFactory, FromArgMatches};
use directories::BaseDirs;
use toml::value::Datetime;
use toml::{Table, Value};
use tracing::debug;

use crate::Cli;

const CONFIG_ENV: &str = "ISOTRAIL_CONFIG";
/// The table every command reads, besides its own
const RENDER: &str = "render";
/// The commands that read config files, each from the table of its name
const COMMANDS: [&str; 4] = [RENDER, "embed", "bundle", "style-lab"];
/// What style-lab takes from [render]: the data settings and the theme, not
/// the looks it lays side by side
const STYLE_LAB_FROM_RENDER: [&str; 13] = [
    "detail",
    "offline",
    "overpass-url",
    "terrain-url",
    "weather-url",
    "cache-dir",
    "no-map-data",
    "padding",
    "footprint",
    "no-weather",
    "season",
    "exaggeration",
    "theme",
];
/// Flags that only make sense for one run
const NOT_SETTINGS: [&str; 8] = [
    "output",
    "config",
    "debug-dir",
    "verbose",
    "quiet",
    "json",
    "help",
    "version",
];

/// The command line, with settings it leaves out taken from the user config
/// file and `--config`, the latter winning, and what was typed; on failure,
/// the error and what was typed
pub fn parse() -> Result<(Cli, Typed), (anyhow::Error, Typed)> {
    let args: Vec<OsString> = env::args_os().collect();
    let matches = matches(&args);
    let typed = Typed::new(&matches);
    match with_config(args, &matches, user_config()) {
        Ok(args) => Ok((parse_args(args), typed)),
        Err(error) => Err((crate::usage(format!("{error:#}")), typed)),
    }
}

fn matches(args: &[OsString]) -> ArgMatches {
    Cli::command()
        .try_get_matches_from(args)
        .unwrap_or_else(|e| explain(e, args).exit())
}

/// Clap's error, with advice for two common slips: a hike file given without
/// a command, and part of an option's name
fn explain(mut error: clap::Error, args: &[OsString]) -> clap::Error {
    let text = |kind| match error.get(kind) {
        Some(ContextValue::String(text)) => Some(text.clone()),
        _ => None,
    };
    match error.kind() {
        ErrorKind::InvalidSubcommand => {
            if let Some(name) = text(ContextKind::InvalidSubcommand)
                .filter(|name| looks_like_a_hike(Path::new(name)))
            {
                let mut cli = Cli::command();
                cli.build();
                return cli.error(
                    ErrorKind::InvalidSubcommand,
                    format!("{name} isn't a command; to make a picture of it, type: isotrail render {name}"),
                );
            }
        }
        ErrorKind::UnknownArgument if error.get(ContextKind::SuggestedArg).is_none() => {
            let typed = text(ContextKind::InvalidArg).unwrap_or_default();
            if let Some(flag) = typed_command(args).and_then(|name| longer_option(&typed, name)) {
                error.insert(ContextKind::SuggestedArg, ContextValue::String(flag));
            }
            // Clap's tip to pass an unknown option as a value never helps here
            if typed.starts_with("--") {
                error.remove(ContextKind::Suggested);
            }
        }
        _ => {}
    }
    error
}

fn looks_like_a_hike(path: &Path) -> bool {
    path.is_file()
        || path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gpx") || ext.eq_ignore_ascii_case("itb"))
}

/// The command named on the command line
fn typed_command(args: &[OsString]) -> Option<String> {
    let cli = Cli::command();
    args.iter()
        .skip(1)
        .filter_map(|arg| arg.to_str())
        .find(|arg| cli.find_subcommand(arg).is_some())
        .map(str::to_owned)
}

/// The option of `command` whose name holds every word of `typed`, such as
/// --route-colour for --colour
fn longer_option(typed: &str, command: String) -> Option<String> {
    let typed = typed.strip_prefix("--")?.split('=').next()?;
    let words: Vec<&str> = typed.split('-').filter(|word| !word.is_empty()).collect();
    if words.is_empty() {
        return None;
    }
    let cli = Cli::command();
    cli.find_subcommand(&command)?
        .get_arguments()
        .chain(cli.get_arguments())
        .filter_map(Arg::get_long)
        .find(|long| {
            let parts: Vec<&str> = long.split('-').collect();
            words.iter().all(|word| parts.contains(word))
        })
        .map(|long| format!("--{long}"))
}

fn parse_args(args: Vec<OsString>) -> Cli {
    Cli::from_arg_matches(&matches(&args)).unwrap_or_else(|e| e.exit())
}

/// The command and the arguments typed for it on the command line, as
/// opposed to those from config files, the environment or defaults
#[derive(Debug, Default)]
pub struct Typed {
    command: Option<String>,
    ids: HashSet<String>,
}

impl Typed {
    fn new(matches: &ArgMatches) -> Typed {
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

/// The user config file, which must exist when `ISOTRAIL_CONFIG` names it
#[derive(Debug)]
struct UserFile {
    path: PathBuf,
    named: bool,
}

/// `ISOTRAIL_CONFIG`, if set and not empty, or `isotrail/config.toml` in
/// the user's config directory
fn user_config() -> Option<UserFile> {
    match env::var_os(CONFIG_ENV) {
        Some(path) => (!path.is_empty()).then(|| UserFile {
            path: PathBuf::from(path),
            named: true,
        }),
        None => BaseDirs::new().map(|dirs| UserFile {
            path: dirs.config_dir().join("isotrail/config.toml"),
            named: false,
        }),
    }
}

/// `args` with a flag added after the command for each setting that isn't
/// already given on the command line or through the environment, and
/// doesn't conflict with a flag that is
fn with_config(
    mut args: Vec<OsString>,
    matches: &ArgMatches,
    user: Option<UserFile>,
) -> anyhow::Result<Vec<OsString>> {
    let Some((name, given)) = matches
        .subcommand()
        .filter(|(name, _)| COMMANDS.contains(name))
    else {
        return Ok(args);
    };
    if let Some(user) = user
        .as_ref()
        .filter(|user| user.named && !user.path.is_file())
    {
        bail!(
            "{CONFIG_ENV} names {}, which does not exist; set it to an empty value for no user config file",
            user.path.display()
        );
    }
    let config = given.get_one::<PathBuf>("config");
    let files: Vec<&Path> = user
        .as_ref()
        .map(|user| user.path.as_path())
        .filter(|path| path.is_file())
        .into_iter()
        .chain(config.map(PathBuf::as_path))
        .collect();

    let cli = Cli::command();
    let command = cli
        .find_subcommand(name)
        .with_context(|| format!("the {name} command is missing"))?;
    let mut settings: BTreeMap<String, (Value, &Path)> = BTreeMap::new();
    for &path in &files {
        let mut tables = read_tables(path, &cli)?;
        let shared = tables.remove(RENDER).unwrap_or_default();
        let own = tables.remove(name).unwrap_or_default();
        for (key, value) in shared {
            if name == RENDER || takes_from_render(name, command, &key, &value) {
                settings.insert(key, (value, path));
            }
        }
        for (key, value) in own {
            settings.insert(key, (value, path));
        }
    }

    let explicit = |arg: &Arg| {
        matches!(
            given.value_source(arg.get_id().as_str()),
            Some(ValueSource::CommandLine | ValueSource::EnvVariable)
        )
    };
    let mut flags = Vec::new();
    for (key, (value, path)) in settings {
        let arg = setting(command, &key).context("a checked setting went missing")?;
        if explicit(arg) {
            continue;
        }
        if let Some(flag) = conflicting(command, arg)
            .into_iter()
            .find(|&other| explicit(other))
        {
            debug!(
                "{key} in {} left out for --{}",
                path.display(),
                flag.get_long().unwrap_or_default()
            );
            continue;
        }
        let in_file = || format!("{key} in {}", path.display());
        let Some(flag) = flag(&key, &value, arg).with_context(in_file)? else {
            continue;
        };
        // Checked alone, so a bad value is blamed on its file
        let alone = [name, "-", flag.as_str()];
        if let Err(error) = command.clone().try_get_matches_from(alone)
            && error.kind() != ErrorKind::MissingRequiredArgument
        {
            // A value our own parser refused says why; clap's text names the flag
            let reason = match std::error::Error::source(&error) {
                Some(reason) if error.kind() == ErrorKind::ValueValidation => reason.to_string(),
                _ => {
                    let message = error.to_string();
                    let first = message.lines().next().unwrap_or_default();
                    first.trim_start_matches("error: ").to_owned()
                }
            };
            bail!("{}: {reason}", in_file());
        }
        flags.push(OsString::from(flag));
    }

    let at = args
        .iter()
        .skip(1)
        .position(|arg| arg == name)
        .with_context(|| format!("the {name} command is missing"))?;
    args.splice(at + 2..at + 2, flags);
    Ok(args)
}

/// The command's flag a setting stands for, if it is one
fn setting<'a>(command: &'a Command, key: &str) -> Option<&'a Arg> {
    command
        .get_arguments()
        .find(|arg| arg.get_long() == Some(key))
        .filter(|_| !NOT_SETTINGS.contains(&key))
}

/// Whether a command other than render takes this [render] setting
fn takes_from_render(name: &str, command: &Command, key: &str, value: &Value) -> bool {
    if setting(command, key).is_none() {
        return false;
    }
    match name {
        "style-lab" => STYLE_LAB_FROM_RENDER.contains(&key),
        // The embed has one shape
        "embed" if key == "aspect" => match value {
            Value::Array(items) => items.len() == 1,
            _ => true,
        },
        _ => true,
    }
}

/// The arguments that can't be given with `arg`, whichever of the two
/// declares the conflict
fn conflicting<'a>(command: &'a Command, arg: &Arg) -> Vec<&'a Arg> {
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

/// A config file's tables by command, each key checked against that
/// command's flags
fn read_tables(path: &Path, cli: &Command) -> anyhow::Result<BTreeMap<String, Table>> {
    let text = crate::read_text(path, "the config file ").map_err(anyhow::Error::msg)?;
    let file: Table = text
        .parse()
        .with_context(|| format!("can't read the config file {}", path.display()))?;
    let mut tables = BTreeMap::new();
    for (name, value) in file {
        let Some(command) = COMMANDS
            .contains(&name.as_str())
            .then(|| cli.find_subcommand(&name))
            .flatten()
        else {
            bail!(
                "{}: settings go in a [render], [embed], [bundle] or [style-lab] table, found {name:?} outside them",
                path.display()
            );
        };
        let Value::Table(table) = value else {
            bail!("{}: [{name}] must be a table", path.display());
        };
        if let Some(key) = table.keys().find(|key| setting(command, key).is_none()) {
            bail!(
                "unknown setting {key:?} in the [{name}] table of {}",
                path.display()
            );
        }
        tables.insert(name, table);
    }
    Ok(tables)
}

/// The flag a setting stands for, or none for a switch that is off
fn flag(key: &str, value: &Value, arg: &Arg) -> anyhow::Result<Option<String>> {
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
fn clock_time(datetime: &Datetime) -> String {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn args(line: &str) -> Vec<OsString> {
        line.split(' ').map(OsString::from).collect()
    }

    fn found(path: PathBuf) -> Option<UserFile> {
        Some(UserFile { path, named: false })
    }

    fn with_config(args: Vec<OsString>, user: Option<UserFile>) -> anyhow::Result<Vec<OsString>> {
        let matches = matches(&args);
        super::with_config(args, &matches, user)
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn flags_beat_the_given_file_which_beats_the_user_file() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[render]\npalette = \"aap-64\"\nscale = 5\nno-attribution = true\ndetail = 64\n",
        );
        let hike = write(
            dir.path(),
            "hike.toml",
            "[render]\npalette = \"resurrect-64\"\naspect = [\"story\", \"wide\"]\n",
        );
        let line = format!(
            "isotrail -v render hike.gpx --scale 6 --config {}",
            hike.display()
        );
        let result = with_config(args(&line), found(user)).unwrap();
        let mut expected = args(&line);
        expected.splice(
            3..3,
            args("--aspect=story,wide --detail=64 --no-attribution --palette=resurrect-64"),
        );
        assert_eq!(strings(&result), strings(&expected));
        // The result parses
        let cli = parse_args(result);
        assert!(matches!(cli.command, crate::Command::Render(_)));
    }

    #[test]
    fn no_files_or_other_commands_leave_the_line_alone() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(dir.path(), "user.toml", "[render]\nscale = 5\n");
        let missing = found(dir.path().join("missing.toml"));
        let line = args("isotrail render hike.gpx");
        assert_eq!(with_config(line.clone(), missing).unwrap(), line);
        let stats = args("isotrail stats hike.gpx");
        assert_eq!(
            with_config(stats.clone(), found(user.clone())).unwrap(),
            stats
        );
        // A file without a [render] table has nothing to add
        let empty = write(dir.path(), "empty.toml", "");
        assert_eq!(with_config(line.clone(), found(empty)).unwrap(), line);
    }

    #[test]
    fn switches_turn_off_in_a_later_file() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(dir.path(), "user.toml", "[render]\noffline = true\n");
        let hike = write(dir.path(), "hike.toml", "[render]\noffline = false\n");
        let line = format!("isotrail render hike.gpx --config {}", hike.display());
        let result = with_config(args(&line), found(user.clone())).unwrap();
        assert_eq!(result, args(&line));
        let result = with_config(args("isotrail render hike.gpx"), found(user)).unwrap();
        assert_eq!(
            strings(&result),
            ["isotrail", "render", "--offline", "hike.gpx"]
        );
    }

    #[test]
    fn on_and_off_settings_take_words_or_booleans() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[render]\npaths = false\nstrata = \"off\"\nshadows = true\nsun = [200, -5]\n",
        );
        let result = with_config(args("isotrail render hike.gpx"), found(user)).unwrap();
        let line = strings(&result);
        for flag in [
            "--paths=off",
            "--strata=off",
            "--shadows=on",
            "--sun=200,-5",
        ] {
            assert!(line.iter().any(|arg| arg == flag), "{flag} in {line:?}");
        }
    }

    #[test]
    fn animation_encoding_and_server_settings_are_keys_and_reports_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[render]
intro = 0
hold = \"2s\"
loop-back = true
orbit = -30
             loop = 3
gif-scale = 1
crf = 23
preset = \"slow\"
             terrain-url = \"https://tiles.example/{z}/{x}/{y}.png\"
             weather-url = \"https://weather.example\"
cache-dir = \"cache\"
",
        );
        let result = with_config(args("isotrail render hike.gpx"), found(user.clone())).unwrap();
        let line = strings(&result);
        for flag in [
            "--intro=0",
            "--hold=2s",
            "--loop-back",
            "--orbit=-30",
            "--loop=3",
            "--gif-scale=1",
            "--crf=23",
            "--preset=slow",
            "--terrain-url=https://tiles.example/{z}/{x}/{y}.png",
            "--weather-url=https://weather.example",
            "--cache-dir=cache",
        ] {
            assert!(line.iter().any(|arg| arg == flag), "{flag} in {line:?}");
        }
        assert!(matches!(
            parse_args(result).command,
            crate::Command::Render(_)
        ));
        // style-lab takes the servers, not the clip settings
        let result = with_config(args("isotrail style-lab hike.gpx -o lab"), found(user)).unwrap();
        let line = strings(&result);
        assert!(
            line.iter().any(|arg| arg == "--cache-dir=cache"),
            "{line:?}"
        );
        assert!(!line.iter().any(|arg| arg.starts_with("--crf")), "{line:?}");
        for key in ["json = true", "quiet = true"] {
            let path = write(
                dir.path(),
                "report.toml",
                &format!(
                    "[render]
{key}
"
                ),
            );
            let error = with_config(args("isotrail render hike.gpx"), found(path)).unwrap_err();
            assert!(error.to_string().contains("unknown setting"), "{error}");
        }
    }

    #[test]
    fn bad_settings_name_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let line = args("isotrail render hike.gpx");
        let error = |text: &str| {
            let path = write(dir.path(), "bad.toml", text);
            format!("{:#}", with_config(line.clone(), found(path)).unwrap_err())
        };
        let message = error("[render]\nshade = 3\n");
        assert!(message.contains("unknown setting \"shade\""), "{message}");
        assert!(message.contains("bad.toml"), "{message}");
        for key in ["output", "config", "file"] {
            let message = error(&format!("[render]\n{key} = \"x\"\n"));
            assert!(message.contains("unknown setting"), "{message}");
        }
        let message = error("[render]\nscale = 17\n");
        assert!(message.starts_with("scale in "), "{message}");
        assert!(
            message.contains("expected a whole number from 1 to 16"),
            "{message}"
        );
        let message = error("[render]\noffline = \"yes\"\n");
        assert!(message.contains("expected true or false"), "{message}");
        let message = error("palette = \"apollo\"\n");
        assert!(message.contains("settings go in a [render]"), "{message}");
        let message = error("[stats]\nunits = \"imperial\"\n");
        assert!(message.contains("found \"stats\" outside"), "{message}");
        let message = error("[embed]\nfps = 5\n");
        assert!(
            message.contains("unknown setting \"fps\" in the [embed] table"),
            "{message}"
        );
        // Another command's table is checked too
        let message = error("[bundle]\npalette = \"apollo\"\n");
        assert!(message.contains("[bundle] table"), "{message}");
        let message = error("[render\n");
        assert!(message.contains("can't read the config file"), "{message}");
    }

    #[test]
    fn a_typed_flag_beats_a_setting_it_conflicts_with() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[render]
date-format = \"short\"
scale = 5
",
        );
        let result = with_config(
            args("isotrail render hike.gpx --date June"),
            found(user.clone()),
        )
        .unwrap();
        assert_eq!(
            strings(&result),
            [
                "isotrail",
                "render",
                "--scale=5",
                "hike.gpx",
                "--date",
                "June"
            ]
        );
        let cli = parse_args(result);
        assert!(matches!(cli.command, crate::Command::Render(_)));
        let result = with_config(args("isotrail render hike.gpx"), found(user)).unwrap();
        assert!(strings(&result).contains(&"--date-format=short".to_owned()));

        let user = write(
            dir.path(),
            "dated.toml",
            "[render]
date = \"June\"
",
        );
        let line = args("isotrail render hike.gpx --date-format iso");
        let result = with_config(line.clone(), found(user)).unwrap();
        assert_eq!(result, line);
    }

    #[test]
    fn part_of_an_option_name_finds_the_whole_one() {
        let render = || Some("render".to_owned());
        assert_eq!(
            longer_option("--colour", render().unwrap()),
            Some("--route-colour".to_owned())
        );
        assert_eq!(
            longer_option("--colour=red", render().unwrap()),
            Some("--route-colour".to_owned())
        );
        assert_eq!(longer_option("--frobnicate", render().unwrap()), None);
        assert_eq!(
            typed_command(&args("isotrail -v render hike.gpx")),
            render()
        );
        assert_eq!(typed_command(&args("isotrail hike.gpx")), None);
    }

    #[test]
    fn a_named_user_file_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let named = Some(UserFile {
            path: dir.path().join("missing.toml"),
            named: true,
        });
        let message = with_config(args("isotrail render hike.gpx"), named)
            .unwrap_err()
            .to_string();
        assert!(message.contains("ISOTRAIL_CONFIG names"), "{message}");
        assert!(message.contains("missing.toml"), "{message}");
        let named = Some(UserFile {
            path: dir.path().join("missing.toml"),
            named: true,
        });
        assert!(with_config(args("isotrail stats hike.gpx"), named).is_ok());
    }

    #[test]
    fn times_can_be_toml_times() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[render]
time = 07:30:00
",
        );
        let result = with_config(args("isotrail render hike.gpx"), found(user)).unwrap();
        assert_eq!(
            strings(&result),
            ["isotrail", "render", "--time=07:30", "hike.gpx"]
        );
        let user = write(
            dir.path(),
            "user.toml",
            "[render]
time = \"20:15\"
",
        );
        let result = with_config(args("isotrail render hike.gpx"), found(user)).unwrap();
        assert_eq!(
            strings(&result),
            ["isotrail", "render", "--time=20:15", "hike.gpx"]
        );
        let user = write(
            dir.path(),
            "user.toml",
            "[render]
time = 07:30:15
",
        );
        let message = format!(
            "{:#}",
            with_config(args("isotrail render hike.gpx"), found(user)).unwrap_err()
        );
        assert!(message.starts_with("time in "), "{message}");
        assert!(message.contains("such as 20:30"), "{message}");
    }

    #[test]
    fn each_command_takes_its_own_table_and_what_it_shares_of_render() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[render]
palette = \"resurrect-64\"
mesher = \"smooth\"
aspect = [\"story\", \"feed\"]
scale = 5
season = \"autumn\"
title = \"Ridge\"
duration = \"5s\"

[embed]
base-url = \"/hikes/\"
loop = \"forever\"
controls = [\"play\", \"scrubber\"]

[style-lab]
palette = [\"apollo\", \"aap-64\"]
",
        );
        let result = |line: &str| strings(&with_config(args(line), found(user.clone())).unwrap());
        assert_eq!(
            result("isotrail embed hike.gpx -o site"),
            [
                "isotrail",
                "embed",
                "--base-url=/hikes/",
                "--controls=play,scrubber",
                "--duration=5s",
                "--loop=forever",
                "--mesher=smooth",
                "--palette=resurrect-64",
                "--scale=5",
                "--season=autumn",
                "--title=Ridge",
                "hike.gpx",
                "-o",
                "site"
            ]
        );
        assert_eq!(
            result("isotrail bundle hike.gpx -o hike.itb"),
            [
                "isotrail",
                "bundle",
                "--season=autumn",
                "--title=Ridge",
                "hike.gpx",
                "-o",
                "hike.itb"
            ]
        );
        assert_eq!(
            result("isotrail style-lab hike.gpx -o lab"),
            [
                "isotrail",
                "style-lab",
                "--palette=apollo,aap-64",
                "--season=autumn",
                "hike.gpx",
                "-o",
                "lab"
            ]
        );
        for line in [
            "isotrail embed hike.gpx -o site",
            "isotrail bundle hike.gpx -o hike.itb",
            "isotrail style-lab hike.gpx -o lab",
            "isotrail render hike.gpx",
        ] {
            let cli = parse_args(with_config(args(line), found(user.clone())).unwrap());
            assert!(!matches!(cli.command, crate::Command::Stats(_)), "{line}");
        }
    }

    #[test]
    fn a_command_table_beats_render_and_embed_takes_one_aspect() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[render]
palette = \"resurrect-64\"
aspect = \"story\"
",
        );
        let hike = write(
            dir.path(),
            "hike.toml",
            "[embed]
palette = \"aap-64\"
",
        );
        let line = format!(
            "isotrail embed hike.gpx -o site --config {}",
            hike.display()
        );
        let result = strings(&with_config(args(&line), found(user)).unwrap());
        assert!(
            result.contains(&"--palette=aap-64".to_owned()),
            "{result:?}"
        );
        assert!(result.contains(&"--aspect=story".to_owned()), "{result:?}");

        // Within one file too, and a list of shapes stays with render
        let both = write(
            dir.path(),
            "both.toml",
            "[embed]
palette = \"apollo\"

[render]
palette = \"aap-64\"
aspect = [\"story\", \"wide\"]
",
        );
        let result = strings(
            &with_config(args("isotrail embed hike.gpx -o site"), found(both.clone())).unwrap(),
        );
        assert_eq!(
            result,
            [
                "isotrail",
                "embed",
                "--palette=apollo",
                "hike.gpx",
                "-o",
                "site"
            ]
        );
        let result = strings(&with_config(args("isotrail render hike.gpx"), found(both)).unwrap());
        assert_eq!(
            result,
            [
                "isotrail",
                "render",
                "--aspect=story,wide",
                "--palette=aap-64",
                "hike.gpx"
            ]
        );
    }

    #[test]
    fn bad_values_for_other_commands_name_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(dir.path(), "user.toml", "[embed]\nscale = 17\n");
        let message = format!(
            "{:#}",
            with_config(args("isotrail embed hike.gpx -o site"), found(user)).unwrap_err()
        );
        assert!(message.starts_with("scale in "), "{message}");
        assert!(
            message.contains("expected a whole number from 1 to 16"),
            "{message}"
        );
    }
}
