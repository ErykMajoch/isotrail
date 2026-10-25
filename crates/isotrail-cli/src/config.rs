use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::parser::ValueSource;
use clap::{Arg, ArgAction, CommandFactory, FromArgMatches};
use directories::BaseDirs;
use toml::{Table, Value};

use crate::Cli;

const CONFIG_ENV: &str = "ISOTRAIL_CONFIG";
const TABLE: &str = "render";
/// Flags that only make sense for one run
const NOT_SETTINGS: [&str; 5] = ["output", "config", "verbose", "help", "version"];

/// The command line, with `render` settings it leaves out taken from the
/// user config file and `--config`, the latter winning
pub fn parse() -> anyhow::Result<Cli> {
    let args = with_config(env::args_os().collect(), user_config())?;
    Ok(parse_args(args))
}

fn parse_args(args: Vec<OsString>) -> Cli {
    let matches = Cli::command()
        .try_get_matches_from(args)
        .unwrap_or_else(|e| e.exit());
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

/// `ISOTRAIL_CONFIG`, if set and not empty, or `isotrail/config.toml` in
/// the user's config directory
fn user_config() -> Option<PathBuf> {
    match env::var_os(CONFIG_ENV) {
        Some(path) => (!path.is_empty()).then(|| PathBuf::from(path)),
        None => BaseDirs::new().map(|dirs| dirs.config_dir().join("isotrail/config.toml")),
    }
}

/// `args` with a flag added after `render` for each setting that isn't
/// already given on the command line or through the environment
fn with_config(mut args: Vec<OsString>, user: Option<PathBuf>) -> anyhow::Result<Vec<OsString>> {
    let command = Cli::command();
    let matches = command
        .clone()
        .try_get_matches_from(&args)
        .unwrap_or_else(|e| e.exit());
    let Some((TABLE, given)) = matches.subcommand() else {
        return Ok(args);
    };
    let config = given.get_one::<PathBuf>("config");
    let files: Vec<&Path> = user
        .as_deref()
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

    let render = command
        .find_subcommand(TABLE)
        .context("the render command is missing")?;
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
        let explicit = matches!(
            given.value_source(arg.get_id().as_str()),
            Some(ValueSource::CommandLine | ValueSource::EnvVariable)
        );
        if explicit {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;

    fn args(line: &str) -> Vec<OsString> {
        line.split(' ').map(OsString::from).collect()
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
        let result = with_config(args(&line), Some(user)).unwrap();
        let mut expected = args(&line);
        expected.splice(
            3..3,
            args("--aspect=story,wide --detail=64 --no-attribution --palette=resurrect-64"),
        );
        assert_eq!(strings(&result), strings(&expected));
        // The result parses
        let cli = parse_args(result);
        assert!(matches!(cli.command, Command::Render(_)));
    }

    #[test]
    fn no_files_or_other_commands_leave_the_line_alone() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(dir.path(), "user.toml", "[render]\nscale = 5\n");
        let missing = Some(dir.path().join("missing.toml"));
        let line = args("isotrail render hike.gpx");
        assert_eq!(with_config(line.clone(), missing).unwrap(), line);
        let stats = args("isotrail stats hike.gpx");
        assert_eq!(
            with_config(stats.clone(), Some(user.clone())).unwrap(),
            stats
        );
        // A file without a [render] table has nothing to add
        let empty = write(dir.path(), "empty.toml", "");
        assert_eq!(with_config(line.clone(), Some(empty)).unwrap(), line);
    }

    #[test]
    fn switches_turn_off_in_a_later_file() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(dir.path(), "user.toml", "[render]\noffline = true\n");
        let hike = write(dir.path(), "hike.toml", "[render]\noffline = false\n");
        let line = format!("isotrail render hike.gpx --config {}", hike.display());
        let result = with_config(args(&line), Some(user.clone())).unwrap();
        assert_eq!(result, args(&line));
        let result = with_config(args("isotrail render hike.gpx"), Some(user)).unwrap();
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
            format!("{:#}", with_config(line.clone(), Some(path)).unwrap_err())
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
}
