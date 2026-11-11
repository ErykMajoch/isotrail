use std::collections::{BTreeMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Command, CommandFactory, FromArgMatches};
use directories::BaseDirs;
use toml::value::Datetime;
use toml::{Table, Value};
use tracing::debug;

use crate::Cli;

const CONFIG_ENV: &str = "ISOTRAIL_CONFIG";
const TABLE: &str = "render";
/// Flags that only make sense for one run
const NOT_SETTINGS: [&str; 5] = ["output", "config", "verbose", "help", "version"];

/// The command line, with `render` settings it leaves out taken from the
/// user config file and `--config`, the latter winning, and what was typed
pub fn parse() -> anyhow::Result<(Cli, Typed)> {
    let args: Vec<OsString> = env::args_os().collect();
    let matches = matches(&args);
    let typed = Typed::new(&matches);
    let args = with_config(args, &matches, user_config())
        .map_err(|error| crate::usage(format!("{error:#}")))?;
    Ok((parse_args(args), typed))
}

fn matches(args: &[OsString]) -> ArgMatches {
    Cli::command()
        .try_get_matches_from(args)
        .unwrap_or_else(|e| e.exit())
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

/// `args` with a flag added after `render` for each setting that isn't
/// already given on the command line or through the environment, and
/// doesn't conflict with a flag that is
fn with_config(
    mut args: Vec<OsString>,
    matches: &ArgMatches,
    user: Option<UserFile>,
) -> anyhow::Result<Vec<OsString>> {
    let Some((TABLE, given)) = matches.subcommand() else {
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

    let mut settings: BTreeMap<String, (Value, &Path)> = BTreeMap::new();
    for &path in &files {
        for (key, value) in read_table(path)? {
            settings.insert(key, (value, path));
        }
    }

    let command = Cli::command();
    let render = command
        .find_subcommand(TABLE)
        .context("the render command is missing")?;
    let explicit = |arg: &Arg| {
        matches!(
            given.value_source(arg.get_id().as_str()),
            Some(ValueSource::CommandLine | ValueSource::EnvVariable)
        )
    };
    let mut flags = Vec::new();
    for (key, (value, path)) in settings {
        let arg = render
            .get_arguments()
            .find(|arg| arg.get_long() == Some(key.as_str()))
            .filter(|_| !NOT_SETTINGS.contains(&key.as_str()))
            .with_context(|| {
                format!(
                    "unknown setting {key:?} in the [{TABLE}] table of {}",
                    path.display()
                )
            })?;
        if explicit(arg) {
            continue;
        }
        if let Some(flag) = conflicting(render, arg)
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
        let alone = ["render", "-", flag.as_str()];
        if let Err(error) = render.clone().try_get_matches_from(alone) {
            let message = error.to_string();
            let first = message.lines().next().unwrap_or_default();
            bail!("{}: {}", in_file(), first.trim_start_matches("error: "));
        }
        flags.push(OsString::from(flag));
    }

    let at = args
        .iter()
        .skip(1)
        .position(|arg| arg == TABLE)
        .context("the render command is missing")?;
    args.splice(at + 2..at + 2, flags);
    Ok(args)
}

/// The arguments that can't be given with `arg`, whichever of the two
/// declares the conflict
fn conflicting<'a>(render: &'a Command, arg: &Arg) -> Vec<&'a Arg> {
    let mut found = render.get_arg_conflicts_with(arg);
    for other in render.get_arguments() {
        let declared = render
            .get_arg_conflicts_with(other)
            .iter()
            .any(|conflict| conflict.get_id() == arg.get_id());
        if declared && !found.iter().any(|f| f.get_id() == other.get_id()) {
            found.push(other);
        }
    }
    found
}

/// The `[render]` table of a config file
fn read_table(path: &Path) -> anyhow::Result<Table> {
    let text =
        fs::read_to_string(path).with_context(|| format!("could not open {}", path.display()))?;
    let mut file: Table = text
        .parse()
        .with_context(|| format!("could not read {}", path.display()))?;
    let table = match file.remove(TABLE) {
        None => Table::new(),
        Some(Value::Table(table)) => table,
        Some(_) => bail!("{}: [{TABLE}] must be a table", path.display()),
    };
    if let Some(key) = file.keys().next() {
        bail!(
            "{}: settings go in a [{TABLE}] table, found {key:?} outside it",
            path.display()
        );
    }
    Ok(table)
}

/// The flag a setting stands for, or none for a switch that is off
fn flag(key: &str, value: &Value, arg: &Arg) -> anyhow::Result<Option<String>> {
    let switch = matches!(arg.get_action(), ArgAction::SetTrue);
    let scalar = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Integer(number) => Some(number.to_string()),
        Value::Float(number) => Some(number.to_string()),
        Value::Datetime(datetime) => Some(clock_time(datetime)),
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
        let message = error("[render]\nscale = 4\n");
        assert!(message.starts_with("scale in "), "{message}");
        assert!(message.contains("must be 2, 3, 5, 6 or 10"), "{message}");
        let message = error("[render]\noffline = \"yes\"\n");
        assert!(message.contains("expected true or false"), "{message}");
        let message = error("palette = \"apollo\"\n");
        assert!(message.contains("[render] table"), "{message}");
        let message = error("[render\n");
        assert!(message.contains("could not read"), "{message}");
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
}
